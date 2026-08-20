use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    Router,
    body::Body,
    http::{HeaderName, HeaderValue, Request, Response, StatusCode, header},
};
use futures::{StreamExt, stream::BoxStream};
use rmcp::{
    model::{
        CallToolRequestParams, ClientCapabilities, ClientInfo, ClientJsonRpcMessage,
        Implementation, ProtocolVersion, ServerJsonRpcMessage,
    },
    service::{ClientLifecycleMode, ClientServiceExt},
    transport::{
        StreamableHttpClientTransport,
        streamable_http_client::{
            StreamableHttpClient, StreamableHttpClientTransportConfig, StreamableHttpError,
            StreamableHttpPostResponse,
        },
    },
};
use sea_orm::{ConnectionTrait, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;
use serde_json::{Value, json};
use sse_stream::{Sse, SseStream};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt as TowerServiceExt;

use crate::{
    app::{RouterConfig, router},
    domain::{AuthConfig, Domain, OAuthConfig, Password, bootstrap_superuser},
    migration::Migrator,
    origin::OriginPolicy,
};

const HOST: &str = "127.0.0.1:3000";
const ISSUER: &str = "https://127.0.0.1:3000";
const RESOURCE: &str = "https://127.0.0.1:3000/mcp";
const MCP_URI: &str = "https://127.0.0.1:3000/mcp";
const EMAIL: &str = "admin@example.com";
const PASSWORD: &str = "c0rrect horse battery staple!";
const CLIENT_ACCEPT: &str = "application/json, text/event-stream";
// Real clients ask for compression. The MCP surface must answer uncompressed,
// because the transport reads the response as a stream.
const CLIENT_ACCEPT_ENCODING: &str = "gzip, br";

struct Fixture {
    db: DatabaseConnection,
    app: Router,
}

async fn fixture() -> Fixture {
    fixture_with_public_url(Some(ISSUER.to_owned())).await
}

async fn fixture_with_public_url(public_url: Option<String>) -> Fixture {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.execute_unprepared("PRAGMA foreign_keys=ON")
        .await
        .unwrap();
    Migrator::up(&db, None).await.unwrap();
    let domain = Arc::new(
        Domain::new(
            db.clone(),
            AuthConfig {
                session_hmac_key: [11; 32],
                session_key_id: "session".into(),
                password_pepper: b"pepper".to_vec(),
                pepper_key_id: "pepper".into(),
                password_concurrency: 1,
                idle_lifetime: Duration::from_secs(60),
                absolute_lifetime: Duration::from_secs(120),
            },
            OAuthConfig {
                hmac_key: [12; 32],
                key_id: "oauth".into(),
            },
        )
        .await
        .unwrap(),
    );
    let password = Password::new(PASSWORD.into()).unwrap();
    bootstrap_superuser(&db, b"pepper", "pepper", EMAIL, &password)
        .await
        .unwrap();
    let app = router(
        domain,
        CancellationToken::new(),
        RouterConfig { public_url },
    );
    Fixture { db, app }
}

fn secure_cookies(public_url: &Option<String>) -> bool {
    OriginPolicy::new(public_url.clone()).secure_cookies()
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, HeaderMapOwned, Vec<u8>) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = HeaderMapOwned::new(&response);
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, headers, bytes)
}

struct HeaderMapOwned(HashMap<String, String>);

impl HeaderMapOwned {
    fn new(response: &Response<Body>) -> Self {
        Self(
            response
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_owned(),
                        String::from_utf8_lossy(value.as_bytes()).into_owned(),
                    )
                })
                .collect(),
        )
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
}

fn get(path: &str) -> Request<Body> {
    Request::get(path)
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap()
}

async fn fetch_json(app: &Router, url: &str) -> Value {
    let path = url.strip_prefix(ISSUER).unwrap_or(url);
    let (status, headers, body) = send(app, get(path)).await;
    assert_eq!(status, StatusCode::OK, "{url} must be fetchable");
    assert!(
        headers
            .get("content-type")
            .is_some_and(|value| value.starts_with("application/json")),
        "{url} must be JSON, got {:?}",
        headers.get("content-type")
    );
    serde_json::from_slice(&body).unwrap()
}

fn mcp_post(token: Option<&str>, body: &Value) -> Request<Body> {
    let mut request = Request::post("/mcp")
        .header(header::HOST, HOST)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, CLIENT_ACCEPT)
        .header(header::ACCEPT_ENCODING, CLIENT_ACCEPT_ENCODING);
    let method = body["method"].as_str().unwrap_or_default();
    let version = body["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"]
        .as_str()
        .or_else(|| body["params"]["protocolVersion"].as_str());
    if let Some(version) = version {
        request = request.header("mcp-protocol-version", version);
        if version >= "2026-07-28" {
            request = request.header("mcp-method", method);
            if let Some(name) = body["params"]["name"]
                .as_str()
                .or_else(|| body["params"]["uri"].as_str())
            {
                request = request.header("mcp-name", name);
            }
        }
    }
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    request
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

fn initialize_body(version: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": version,
            "capabilities": {},
            "clientInfo": {"name": "conformance", "version": "1"}
        }
    })
}

fn call_body(id: u32, method: &str, params: Value) -> Value {
    let mut params = params;
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "conformance", "version": "1"},
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

fn jsonrpc_message(headers: &HeaderMapOwned, body: &[u8]) -> Value {
    assert!(
        headers.get("content-encoding").is_none(),
        "an MCP response must not be compressed, got {:?}",
        headers.get("content-encoding")
    );
    let content_type = headers.get("content-type").unwrap_or_default();
    let text = std::str::from_utf8(body).unwrap();
    assert!(
        !content_type.starts_with("text/html"),
        "an MCP response must never be HTML: {text}"
    );
    if content_type.starts_with("text/event-stream") {
        let data = text
            .lines()
            .find_map(|line| line.strip_prefix("data:"))
            .expect("an SSE response carries a data line")
            .trim();
        serde_json::from_str(data).unwrap()
    } else {
        serde_json::from_slice(body).unwrap()
    }
}

fn set_cookies(response: &Response<Body>) -> Vec<String> {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or_default().to_owned())
        .collect()
}

async fn oauth_dance(app: &Router, scope: &str) -> String {
    let protected = fetch_json(app, "/.well-known/oauth-protected-resource/mcp").await;
    let authorization_server = protected["authorization_servers"][0].as_str().unwrap();
    let metadata = fetch_json(
        app,
        &format!("{authorization_server}/.well-known/oauth-authorization-server"),
    )
    .await;
    let registration_endpoint = metadata["registration_endpoint"]
        .as_str()
        .unwrap()
        .to_owned();
    let authorization_endpoint = metadata["authorization_endpoint"]
        .as_str()
        .unwrap()
        .to_owned();
    let token_endpoint = metadata["token_endpoint"].as_str().unwrap().to_owned();

    let (status, _, body) = send(
        app,
        Request::post(path_of(&registration_endpoint))
            .header(header::HOST, HOST)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "redirect_uris": ["http://127.0.0.1:49152/callback"],
                    "client_name": "conformance",
                    "application_type": "native",
                    "token_endpoint_auth_method": "none",
                    "grant_types": ["authorization_code"],
                    "response_types": ["code"],
                    "scope": scope
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let registration: Value = serde_json::from_slice(&body).unwrap();
    let client_id = registration["client_id"].as_str().unwrap().to_owned();

    let verifier = "correct-verifier-with-at-least-forty-three-characters-123";
    let challenge = {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        use sha2::{Digest, Sha256};
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
    };
    let mut authorize = url::Url::parse(&authorization_endpoint).unwrap();
    authorize
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", "http://127.0.0.1:49152/callback")
        .append_pair("scope", scope)
        .append_pair("state", "conformance-state")
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("resource", RESOURCE);
    let authorize_path = authorize[url::Position::BeforePath..].to_owned();

    let anonymous = app.clone().oneshot(get(&authorize_path)).await.unwrap();
    assert_eq!(anonymous.status(), StatusCode::SEE_OTHER);
    let login_path = anonymous.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_owned();

    let login_page = app.clone().oneshot(get(&login_path)).await.unwrap();
    assert_eq!(login_page.status(), StatusCode::OK);
    let login_cookie = set_cookies(&login_page)
        .into_iter()
        .find(|cookie| cookie.contains("frater_csrf="))
        .expect("the login page sets a CSRF cookie");
    let login_csrf = login_cookie.split_once('=').unwrap().1.to_owned();
    let login_form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("email", EMAIL)
        .append_pair("password", PASSWORD)
        .append_pair("csrf", &login_csrf)
        .append_pair("return_to", &authorize_path)
        .finish();
    let login = app
        .clone()
        .oneshot(
            Request::post("/login")
                .header(header::HOST, HOST)
                .header(header::ORIGIN, ISSUER)
                .header(header::COOKIE, &login_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(login_form))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::SEE_OTHER);
    let cookies = set_cookies(&login).join("; ");
    let csrf = cookies
        .split("; ")
        .find_map(|pair| pair.split_once('='))
        .filter(|(name, _)| name.contains("csrf"))
        .map(|(_, value)| value.to_owned())
        .or_else(|| {
            cookies
                .split("; ")
                .filter_map(|pair| pair.split_once('='))
                .find(|(name, _)| name.contains("csrf"))
                .map(|(_, value)| value.to_owned())
        })
        .expect("the session carries a CSRF cookie");

    let consent = app
        .clone()
        .oneshot(
            Request::get(&authorize_path)
                .header(header::HOST, HOST)
                .header(header::COOKIE, &cookies)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(consent.status(), StatusCode::OK);

    let decision = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("csrf", &csrf)
        .append_pair("decision", "allow")
        .finish();
    let granted = app
        .clone()
        .oneshot(
            Request::post(&authorize_path)
                .header(header::HOST, HOST)
                .header(header::ORIGIN, ISSUER)
                .header(header::COOKIE, &cookies)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(decision))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(granted.status(), StatusCode::SEE_OTHER);
    let redirect = url::Url::parse(granted.headers()[header::LOCATION].to_str().unwrap()).unwrap();
    let parameters: HashMap<_, _> = redirect.query_pairs().into_owned().collect();
    let code = parameters.get("code").expect("the redirect carries a code");

    let token_form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("code", code)
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", "http://127.0.0.1:49152/callback")
        .append_pair("code_verifier", verifier)
        .append_pair("resource", RESOURCE)
        .finish();
    let (status, _, body) = send(
        app,
        Request::post(path_of(&token_endpoint))
            .header(header::HOST, HOST)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(token_form))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let issued: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(issued["token_type"], "Bearer");
    issued["access_token"].as_str().unwrap().to_owned()
}

fn path_of(url: &str) -> String {
    url::Url::parse(url)
        .map(|parsed| parsed[url::Position::BeforePath..].to_owned())
        .unwrap_or_else(|_| url.to_owned())
}

#[derive(Clone)]
struct RouterClient {
    app: Router,
    token: Arc<str>,
}

#[derive(Debug, thiserror::Error)]
enum RouterClientError {
    #[error("http error: {0}")]
    Http(String),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

impl StreamableHttpClient for RouterClient {
    type Error = RouterClientError;

    async fn post_message(
        &self,
        _uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        _auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        let payload = serde_json::to_vec(&message)
            .map_err(|error| StreamableHttpError::Client(RouterClientError::Json(error)))?;
        let mut request = Request::post("/mcp")
            .header(header::HOST, HOST)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, CLIENT_ACCEPT)
            .header(header::ACCEPT_ENCODING, CLIENT_ACCEPT_ENCODING)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.token));
        for (name, value) in custom_headers {
            request = request.header(name, value);
        }
        if let Some(session_id) = session_id {
            request = request.header("mcp-session-id", session_id.as_ref());
        }
        let response = self
            .app
            .clone()
            .oneshot(request.body(Body::from(payload)).unwrap())
            .await
            .map_err(|error| {
                StreamableHttpError::Client(RouterClientError::Http(error.to_string()))
            })?;
        let status = response.status();
        let session_id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        assert!(
            !response.headers().contains_key(header::CONTENT_ENCODING),
            "the MCP surface must answer a compressing client uncompressed"
        );
        if matches!(status, StatusCode::ACCEPTED | StatusCode::NO_CONTENT) {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        if !status.is_success() {
            let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default();
            return Err(StreamableHttpError::Client(RouterClientError::Http(
                format!("HTTP {status}: {body}"),
            )));
        }
        if content_type.starts_with("text/event-stream") {
            let stream: BoxStream<'static, Result<Sse, sse_stream::Error>> =
                SseStream::from_bytes_stream(response.into_body().into_data_stream()).boxed();
            return Ok(StreamableHttpPostResponse::Sse(stream, session_id));
        }
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .map_err(|error| {
                StreamableHttpError::Client(RouterClientError::Http(error.to_string()))
            })?;
        if bytes.is_empty() {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        let message: ServerJsonRpcMessage = serde_json::from_slice(&bytes)
            .map_err(|error| StreamableHttpError::Client(RouterClientError::Json(error)))?;
        Ok(StreamableHttpPostResponse::Json(message, session_id))
    }

    async fn delete_session(
        &self,
        _uri: Arc<str>,
        _session_id: Arc<str>,
        _auth_header: Option<String>,
        _custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), StreamableHttpError<Self::Error>> {
        Ok(())
    }

    async fn get_stream(
        &self,
        _uri: Arc<str>,
        _session_id: Option<Arc<str>>,
        _last_event_id: Option<String>,
        _auth_header: Option<String>,
        _custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, sse_stream::Error>>, StreamableHttpError<Self::Error>>
    {
        Err(StreamableHttpError::UnexpectedServerResponse(
            "the server does not offer a standalone SSE stream".into(),
        ))
    }
}

fn call_tool_params(name: &'static str) -> CallToolRequestParams {
    let mut params = CallToolRequestParams::default();
    params.name = name.into();
    params.arguments = Some(serde_json::Map::new());
    params
}

#[tokio::test]
async fn unauthenticated_initialize_returns_a_typed_challenge_with_reachable_metadata() {
    let fixture = fixture().await;
    let (status, headers, body) =
        send(&fixture.app, mcp_post(None, &initialize_body("2026-07-28"))).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let challenge = headers
        .get("www-authenticate")
        .expect("a 401 carries a bearer challenge");
    assert!(challenge.starts_with("Bearer "));
    let metadata_url = challenge
        .split_once("resource_metadata=\"")
        .map(|(_, rest)| rest.split('"').next().unwrap().to_owned())
        .expect("the challenge carries resource_metadata");
    assert_eq!(
        metadata_url,
        format!("{ISSUER}/.well-known/oauth-protected-resource/mcp")
    );

    assert!(
        headers
            .get("content-type")
            .is_some_and(|value| value.starts_with("application/json")),
        "the challenge body must be JSON, got {:?}",
        headers.get("content-type")
    );
    let error: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(error["error"], "unauthorized");
    assert_eq!(error["resource_metadata"], metadata_url);

    let protected = fetch_json(&fixture.app, &metadata_url).await;
    assert_eq!(protected["resource"], RESOURCE);
    let authorization_server = protected["authorization_servers"][0].as_str().unwrap();
    assert_eq!(authorization_server, ISSUER);

    let metadata = fetch_json(
        &fixture.app,
        &format!("{authorization_server}/.well-known/oauth-authorization-server"),
    )
    .await;
    assert_eq!(metadata["issuer"], ISSUER);
    for key in [
        "authorization_endpoint",
        "token_endpoint",
        "registration_endpoint",
    ] {
        let endpoint = metadata[key].as_str().unwrap();
        assert!(
            endpoint.starts_with(ISSUER),
            "{key} must live on the issuer, got {endpoint}"
        );
        let (status, _, _) = send(&fixture.app, get(&path_of(endpoint))).await;
        assert_ne!(
            status,
            StatusCode::NOT_FOUND,
            "{key} ({endpoint}) must resolve on the router"
        );
    }
    fixture.db.close().await.unwrap();
}

#[tokio::test]
async fn rmcp_client_completes_the_whole_connect_path_over_the_router() {
    let fixture = fixture().await;
    let token = oauth_dance(&fixture.app, "workouts:read catalogue:read").await;

    let client = RouterClient {
        app: fixture.app.clone(),
        token: token.into(),
    };
    let transport = StreamableHttpClientTransport::with_client(
        client,
        StreamableHttpClientTransportConfig::with_uri(MCP_URI),
    );
    let mut client_info = ClientInfo::default();
    client_info.protocol_version = ProtocolVersion::V_2026_07_28;
    client_info.capabilities = ClientCapabilities::default();
    client_info.client_info = Implementation::new("conformance", "1");
    let service = client_info
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .expect("a real rmcp client must complete the handshake");

    let server_info = service.peer_info().expect("initialize returns server info");
    assert_eq!(
        server_info.protocol_version,
        ProtocolVersion::V_2026_07_28,
        "the negotiated version must be the one the server implements"
    );
    assert_eq!(
        server_info.server_info.as_ref().unwrap().name,
        env!("CARGO_PKG_NAME")
    );
    assert!(server_info.capabilities.tools.is_some());
    assert!(server_info.capabilities.resources.is_some());

    let tools = service.list_tools(None).await.unwrap();
    assert!(!tools.tools.is_empty());
    assert!(tools.tools.iter().any(|tool| tool.name == "list_muscles"));

    let called = service
        .call_tool(call_tool_params("list_muscles"))
        .await
        .unwrap();
    assert_ne!(called.is_error, Some(true));

    let resources = service.list_resources(None).await.unwrap();
    assert!(!resources.resources.is_empty());

    service.cancel().await.unwrap();
    fixture.db.close().await.unwrap();
}

#[tokio::test]
async fn initialize_negotiates_the_one_supported_version() {
    let fixture = fixture().await;
    let token = oauth_dance(&fixture.app, "workouts:read").await;

    let (status, headers, body) = send(
        &fixture.app,
        mcp_post(Some(&token), &initialize_body("2026-07-28")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let message = jsonrpc_message(&headers, &body);
    assert_eq!(message["result"]["protocolVersion"], "2026-07-28");
    assert_eq!(
        message["result"]["serverInfo"]["name"],
        env!("CARGO_PKG_NAME")
    );
    assert!(message["result"]["capabilities"]["tools"].is_object());

    for offered in ["2025-06-18", "2025-11-25", "1999-01-01"] {
        let (status, headers, body) = send(
            &fixture.app,
            mcp_post(Some(&token), &initialize_body(offered)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "offer {offered} must not fail");
        let message = jsonrpc_message(&headers, &body);
        assert_eq!(
            message["result"]["protocolVersion"], "2026-07-28",
            "offer {offered} must get the server's supported version"
        );
    }
    fixture.db.close().await.unwrap();
}

#[tokio::test]
async fn the_post_initialize_calls_a_client_makes_all_succeed() {
    let fixture = fixture().await;
    let token = oauth_dance(&fixture.app, "workouts:read catalogue:read").await;

    let (status, headers, _) = send(
        &fixture.app,
        mcp_post(Some(&token), &initialize_body("2026-07-28")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get("mcp-session-id"), None);

    let notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {"_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28"}}
    });
    let (status, _, _) = send(&fixture.app, mcp_post(Some(&token), &notification)).await;
    assert!(
        status == StatusCode::ACCEPTED || status == StatusCode::OK,
        "notifications/initialized must be accepted, got {status}"
    );

    let (status, headers, body) = send(
        &fixture.app,
        mcp_post(Some(&token), &call_body(2, "tools/list", json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let tools = jsonrpc_message(&headers, &body);
    assert!(
        !tools["result"]["tools"].as_array().unwrap().is_empty(),
        "tools/list must not be empty"
    );

    let (status, headers, body) = send(
        &fixture.app,
        mcp_post(
            Some(&token),
            &call_body(
                3,
                "tools/call",
                json!({"name": "list_muscles", "arguments": {}}),
            ),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let called = jsonrpc_message(&headers, &body);
    assert_ne!(called["result"]["isError"], true);

    let (status, headers, body) = send(
        &fixture.app,
        mcp_post(Some(&token), &call_body(4, "resources/list", json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resources = jsonrpc_message(&headers, &body);
    assert!(
        !resources["result"]["resources"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    fixture.db.close().await.unwrap();
}

#[tokio::test]
async fn the_mcp_surface_never_compresses_its_answer() {
    let fixture = fixture().await;
    let token = oauth_dance(&fixture.app, "workouts:read").await;

    for request in [
        mcp_post(None, &initialize_body("2026-07-28")),
        mcp_post(Some(&token), &initialize_body("2026-07-28")),
        mcp_post(Some(&token), &call_body(2, "tools/list", json!({}))),
    ] {
        let (_, headers, _) = send(&fixture.app, request).await;
        assert_eq!(
            headers.get("content-encoding"),
            None,
            "no MCP response may be compressed"
        );
    }

    let (status, headers, _) = send(
        &fixture.app,
        Request::get("/")
            .header(header::HOST, HOST)
            .header(header::ACCEPT_ENCODING, CLIENT_ACCEPT_ENCODING)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.get("content-encoding").is_some());
    fixture.db.close().await.unwrap();
}

#[tokio::test]
async fn a_wrong_or_revoked_token_gets_a_typed_challenge_and_never_html() {
    let fixture = fixture().await;
    let token = oauth_dance(&fixture.app, "workouts:read").await;

    for authorization in ["Bearer not-a-token", "Bearer", "Basic dXNlcjpwYXNz"] {
        let mut request = mcp_post(None, &initialize_body("2026-07-28"));
        request.headers_mut().insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(authorization).unwrap(),
        );
        let (status, headers, body) = send(&fixture.app, request).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{authorization} must give 401"
        );
        assert!(
            headers
                .get("www-authenticate")
                .unwrap()
                .starts_with("Bearer ")
        );
        assert!(
            headers
                .get("content-type")
                .is_some_and(|value| value.starts_with("application/json")),
            "{authorization} must give a typed body"
        );
        let error: Value = serde_json::from_slice(&body).unwrap();
        assert!(error["error"].is_string());
    }

    fixture
        .db
        .execute_unprepared("DELETE FROM oauth_access_tokens")
        .await
        .unwrap();
    let (status, headers, body) = send(
        &fixture.app,
        mcp_post(Some(&token), &initialize_body("2026-07-28")),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        headers
            .get("www-authenticate")
            .unwrap()
            .contains("error=\"invalid_token\"")
    );
    let error: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(error["error"], "invalid_token");
    fixture.db.close().await.unwrap();
}

#[tokio::test]
async fn a_token_whose_scopes_no_longer_exist_gets_a_clean_challenge() {
    let fixture = fixture().await;
    let token = oauth_dance(&fixture.app, "workouts:read").await;

    fixture
        .db
        .execute_unprepared("UPDATE oauth_access_tokens SET scope='frater:read frater:write'")
        .await
        .unwrap();
    let (status, headers, body) = send(
        &fixture.app,
        mcp_post(Some(&token), &initialize_body("2026-07-28")),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let challenge = headers.get("www-authenticate").unwrap();
    assert!(challenge.contains("error=\"insufficient_scope\""));
    assert!(challenge.contains("scope=\"workouts:read\""));
    let error: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(error["error"], "insufficient_scope");
    fixture.db.close().await.unwrap();
}

#[tokio::test]
async fn a_post_without_both_accept_types_is_refused_in_plain_text() {
    let fixture = fixture().await;
    let token = oauth_dance(&fixture.app, "workouts:read").await;

    for accept in [None, Some("application/json"), Some("text/event-stream")] {
        let mut request = mcp_post(Some(&token), &initialize_body("2026-07-28"));
        request.headers_mut().remove(header::ACCEPT);
        if let Some(accept) = accept {
            request
                .headers_mut()
                .insert(header::ACCEPT, HeaderValue::from_static(accept));
        }
        let (status, headers, body) = send(&fixture.app, request).await;
        assert_eq!(
            status,
            StatusCode::NOT_ACCEPTABLE,
            "accept {accept:?} must be refused"
        );
        let text = String::from_utf8_lossy(&body);
        assert!(
            !headers
                .get("content-type")
                .unwrap_or_default()
                .starts_with("text/html"),
            "the refusal must not be HTML: {text}"
        );
        assert!(text.contains("Not Acceptable"));
    }
    fixture.db.close().await.unwrap();
}

#[tokio::test]
async fn get_mcp_needs_auth_and_then_reports_that_only_post_is_allowed() {
    let fixture = fixture().await;
    let token = oauth_dance(&fixture.app, "workouts:read").await;

    let (status, headers, _) = send(
        &fixture.app,
        Request::get("/mcp")
            .header(header::HOST, HOST)
            .header(header::ACCEPT, "text/event-stream")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        headers
            .get("www-authenticate")
            .unwrap()
            .starts_with("Bearer ")
    );

    let (status, headers, body) = send(
        &fixture.app,
        Request::get("/mcp")
            .header(header::HOST, HOST)
            .header(header::ACCEPT, "text/event-stream")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(headers.get("allow"), Some("POST"));
    assert!(!String::from_utf8_lossy(&body).contains("<html"));
    fixture.db.close().await.unwrap();
}

#[tokio::test]
async fn an_origin_from_another_site_is_rejected_and_the_matching_one_passes() {
    let fixture = fixture().await;
    let token = oauth_dance(&fixture.app, "workouts:read").await;

    let with_origin = |origin: &'static str| {
        let mut request = mcp_post(Some(&token), &initialize_body("2026-07-28"));
        request
            .headers_mut()
            .insert(header::ORIGIN, HeaderValue::from_static(origin));
        request
    };
    let (status, _, _) = send(&fixture.app, with_origin("https://evil.example")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, headers, body) = send(&fixture.app, with_origin(ISSUER)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        jsonrpc_message(&headers, &body)["result"]["protocolVersion"],
        "2026-07-28"
    );
    fixture.db.close().await.unwrap();
}

#[tokio::test]
async fn a_deployment_without_a_public_url_accepts_its_own_host() {
    let public_url = None;
    assert!(!secure_cookies(&public_url));
    let fixture = fixture_with_public_url(public_url).await;

    let (status, headers, _) =
        send(&fixture.app, mcp_post(None, &initialize_body("2026-07-28"))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        headers
            .get("www-authenticate")
            .unwrap()
            .contains("http://127.0.0.1:3000/.well-known/oauth-protected-resource/mcp"),
        "the challenge must name the origin of the request"
    );

    let mut matching = mcp_post(None, &initialize_body("2026-07-28"));
    matching.headers_mut().insert(
        header::ORIGIN,
        HeaderValue::from_static("http://127.0.0.1:3000"),
    );
    let (status, _, _) = send(&fixture.app, matching).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a matching origin passes the origin check and stops at the token check"
    );

    let mut foreign = mcp_post(None, &initialize_body("2026-07-28"));
    foreign.headers_mut().insert(
        header::ORIGIN,
        HeaderValue::from_static("http://evil.example"),
    );
    let (status, _, _) = send(&fixture.app, foreign).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    fixture.db.close().await.unwrap();
}

#[tokio::test]
async fn the_request_contract_a_client_must_meet_after_the_handshake() {
    let fixture = fixture().await;
    let token = oauth_dance(&fixture.app, "workouts:read").await;

    let (status, headers, body) = send(
        &fixture.app,
        mcp_post(Some(&token), &initialize_body("2026-07-28")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        jsonrpc_message(&headers, &body)["result"]["protocolVersion"],
        "2026-07-28"
    );

    let request = Request::post("/mcp")
        .header(header::HOST, HOST)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, CLIENT_ACCEPT)
        .header(header::ACCEPT_ENCODING, CLIENT_ACCEPT_ENCODING)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", "tools/list")
        .body(Body::from(
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}).to_string(),
        ))
        .unwrap();
    let (status, headers, body) = send(&fixture.app, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let error = jsonrpc_message(&headers, &body);
    assert_eq!(error["error"]["code"], -32602);
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("_meta is missing")
    );

    let request = Request::post("/mcp")
        .header(header::HOST, HOST)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, CLIENT_ACCEPT)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": {}}).to_string(),
        ))
        .unwrap();
    let (status, headers, body) = send(&fixture.app, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let error = jsonrpc_message(&headers, &body);
    assert_eq!(error["error"]["code"], -32020);

    let (status, headers, body) = send(
        &fixture.app,
        mcp_post(Some(&token), &call_body(4, "tools/list", json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !jsonrpc_message(&headers, &body)["result"]["tools"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    fixture.db.close().await.unwrap();
}

#[tokio::test]
async fn the_discover_document_names_the_one_supported_revision() {
    let fixture = fixture().await;
    let token = oauth_dance(&fixture.app, "workouts:read").await;

    let (status, headers, body) = send(
        &fixture.app,
        mcp_post(Some(&token), &call_body(1, "server/discover", json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let discovered = jsonrpc_message(&headers, &body);
    assert_eq!(
        discovered["result"]["supportedVersions"],
        json!(["2026-07-28"])
    );
    assert!(discovered["result"]["capabilities"]["tools"].is_object());
    fixture.db.close().await.unwrap();
}
