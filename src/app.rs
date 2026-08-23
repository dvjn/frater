use crate::{access_log, domain::Domain, mcp, origin::OriginPolicy, request_id, web};
use axum::{Router, middleware};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct RouterConfig {
    pub public_url: Option<String>,
}

#[derive(Clone)]
pub struct AppState {
    pub domain: Arc<crate::domain::Domain>,
    pub origin: OriginPolicy,
}

pub fn router(
    domain: Arc<Domain>,
    cancellation: CancellationToken,
    config: RouterConfig,
) -> Router {
    let origin = OriginPolicy::new(config.public_url);
    let state = AppState { domain, origin };
    // Every subsystem keeps its own router, so that one access log layer, and
    // thus one `RUST_LOG` target, covers exactly that subsystem.
    Router::new()
        .merge(web::auth_router(state.clone()).layer(access_log::layer!(access_log::AUTH)))
        .merge(web::oauth_router(state.clone()).layer(access_log::layer!(access_log::OAUTH)))
        .merge(web::asset_router().layer(access_log::layer!(access_log::ASSETS)))
        .merge(web::healthz_router(state.clone()).layer(access_log::layer!(access_log::HEALTHZ)))
        // MCP carries no web timeout. The ordinary timeout must not stop a
        // long-lived SSE response.
        .merge(mcp::router(state, cancellation).layer(access_log::layer!(access_log::MCP)))
        // The id layer stays outermost, so the span, the handler and the error
        // body all read the same value.
        .layer(middleware::from_fn(request_id::propagate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            AuthConfig, AuthorizationCodeRedemption, AuthorizationCodeRequest, ClientRegistration,
            CreateWorkoutSession, Domain, Identity, OAuthConfig, Password, bootstrap_superuser,
            test_oauth_principal,
        },
        migration::Migrator,
    };
    use axum::response::Response;
    use axum::{
        body::Body,
        http::{HeaderValue, Request, StatusCode, header},
    };
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use sea_orm::{ConnectionTrait, Database};
    use sea_orm_migration::MigratorTrait;
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use std::time::Duration;
    use tower::ServiceExt;

    fn local_config() -> RouterConfig {
        RouterConfig { public_url: None }
    }

    async fn mcp_fixture() -> (sea_orm::DatabaseConnection, Arc<Domain>, String) {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared("PRAGMA foreign_keys=ON")
            .await
            .unwrap();
        Migrator::up(&db, None).await.unwrap();
        let domain = Arc::new(
            Domain::new(
                db.clone(),
                AuthConfig {
                    session_hmac_key: [1; 32],
                    session_key_id: "session".into(),
                    password_pepper: b"pepper".to_vec(),
                    pepper_key_id: "pepper".into(),
                    password_concurrency: 2,
                    idle_lifetime: Duration::from_secs(60),
                    absolute_lifetime: Duration::from_secs(120),
                },
                OAuthConfig {
                    hmac_key: [2; 32],
                    key_id: "oauth".into(),
                },
            )
            .await
            .unwrap(),
        );
        let password = Password::new("c0rrect horse battery staple!".into()).unwrap();
        bootstrap_superuser(&db, b"pepper", "pepper", "admin@example.com", &password)
            .await
            .unwrap();
        let identity = domain
            .auth()
            .verify_password_identity("admin@example.com", &password)
            .await
            .unwrap();
        let client = domain
            .oauth()
            .register_public_client(ClientRegistration {
                issuer: "http://127.0.0.1:3000".into(),
                redirect_uris: vec!["http://127.0.0.1:49152/callback".into()],
                client_name: None,
                application_type: Some("native".into()),
                grant_types: vec!["authorization_code".into()],
                response_types: vec!["code".into()],
                scope: "workouts:read".into(),
            })
            .await
            .unwrap();
        let verifier = "correct-verifier-with-at-least-forty-three-characters-123";
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let code = domain
            .oauth()
            .issue_authorization_code(AuthorizationCodeRequest {
                identity: &identity,
                client_id: client.client_id(),
                issuer: "http://127.0.0.1:3000",
                redirect_uri: "http://127.0.0.1:49152/callback",
                resource: "http://127.0.0.1:3000/mcp",
                scope: "workouts:read",
                code_challenge: &challenge,
            })
            .await
            .unwrap();
        let token = domain
            .oauth()
            .redeem_authorization_code(AuthorizationCodeRedemption {
                code: code.expose(),
                client_id: client.client_id(),
                issuer: "http://127.0.0.1:3000",
                redirect_uri: "http://127.0.0.1:49152/callback",
                resource: "http://127.0.0.1:3000/mcp",
                code_verifier: verifier,
            })
            .await
            .unwrap()
            .expose()
            .to_owned();
        (db, domain, token)
    }

    fn initialize_request(authorization: Option<&str>) -> Request<Body> {
        mcp_request(
            authorization,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2026-07-28",
                    "capabilities": {},
                    "clientInfo": {"name": "test", "version": "1"}
                }
            }),
        )
    }

    fn mcp_request(authorization: Option<&str>, mut body: Value) -> Request<Body> {
        let method = body["method"].as_str().unwrap_or_default().to_owned();
        let name = body["params"]["name"]
            .as_str()
            .or_else(|| body["params"]["uri"].as_str())
            .map(str::to_owned);
        if method != "initialize"
            && let Some(params) = body.get_mut("params").and_then(Value::as_object_mut)
        {
            params.insert(
                "_meta".into(),
                json!({
                    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                    "io.modelcontextprotocol/clientCapabilities": {}
                }),
            );
        }
        let mut request = Request::post("/mcp")
            .header(header::HOST, "127.0.0.1:3000")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header("mcp-protocol-version", "2026-07-28")
            .header("mcp-method", method);
        if let Some(name) = name {
            request = request.header("mcp-name", name);
        }
        if let Some(authorization) = authorization {
            request = request.header(header::AUTHORIZATION, authorization);
        }
        request
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    }

    async fn response_json(response: Response) -> Value {
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert!(
            matches!(status, StatusCode::OK | StatusCode::BAD_REQUEST),
            "unexpected MCP response {status}: {}",
            String::from_utf8_lossy(&bytes)
        );
        if content_type.starts_with("text/event-stream") {
            let text = std::str::from_utf8(&bytes).unwrap();
            let data = text
                .lines()
                .find_map(|line| line.strip_prefix("data:"))
                .expect("SSE response has a data line")
                .trim();
            serde_json::from_str(data).unwrap()
        } else {
            serde_json::from_slice(&bytes).unwrap()
        }
    }

    async fn issue_token(domain: &Domain, identity: &Identity, scope: &str) -> String {
        let client = domain
            .oauth()
            .register_public_client(ClientRegistration {
                issuer: "http://127.0.0.1:3000".into(),
                redirect_uris: vec!["http://127.0.0.1:49152/callback".into()],
                client_name: None,
                application_type: Some("native".into()),
                grant_types: vec!["authorization_code".into()],
                response_types: vec!["code".into()],
                scope: scope.into(),
            })
            .await
            .unwrap();
        let verifier = "correct-verifier-with-at-least-forty-three-characters-123";
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let code = domain
            .oauth()
            .issue_authorization_code(AuthorizationCodeRequest {
                identity,
                client_id: client.client_id(),
                issuer: "http://127.0.0.1:3000",
                redirect_uri: "http://127.0.0.1:49152/callback",
                resource: "http://127.0.0.1:3000/mcp",
                scope,
                code_challenge: &challenge,
            })
            .await
            .unwrap();
        domain
            .oauth()
            .redeem_authorization_code(AuthorizationCodeRedemption {
                code: code.expose(),
                client_id: client.client_id(),
                issuer: "http://127.0.0.1:3000",
                redirect_uri: "http://127.0.0.1:49152/callback",
                resource: "http://127.0.0.1:3000/mcp",
                code_verifier: verifier,
            })
            .await
            .unwrap()
            .expose()
            .to_owned()
    }

    struct ProtocolFixture {
        db: sea_orm::DatabaseConnection,
        domain: Arc<Domain>,
        read_token: String,
        workouts_only_token: String,
        write_token: String,
        admin_token: String,
        foreign_session_id: uuid::Uuid,
    }

    async fn protocol_fixture() -> ProtocolFixture {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared("PRAGMA foreign_keys=ON")
            .await
            .unwrap();
        Migrator::up(&db, None).await.unwrap();
        let super_id = uuid::Uuid::now_v7();
        let other_id = uuid::Uuid::now_v7();
        for (id, email, role) in [
            (super_id, "root@example.com", "superuser"),
            (other_id, "other@example.com", "user"),
        ] {
            db.execute_unprepared(&format!(
                "INSERT INTO users(id,email_normalized,email_display,role,status,auth_version,created_at,updated_at) VALUES('{id}','{email}','{email}','{role}','active',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')"
            ))
            .await
            .unwrap();
        }
        let domain = Arc::new(
            Domain::new(
                db.clone(),
                AuthConfig {
                    session_hmac_key: [7; 32],
                    session_key_id: "session".into(),
                    password_pepper: b"pepper".to_vec(),
                    pepper_key_id: "pepper".into(),
                    password_concurrency: 1,
                    idle_lifetime: Duration::from_secs(60),
                    absolute_lifetime: Duration::from_secs(120),
                },
                OAuthConfig {
                    hmac_key: [9; 32],
                    key_id: "oauth".into(),
                },
            )
            .await
            .unwrap(),
        );
        let identity = Identity {
            user_id: super_id,
            role: "superuser".into(),
            auth_version: 0,
        };
        let read_token = issue_token(&domain, &identity, "workouts:read catalogue:read").await;
        let workouts_only_token = issue_token(&domain, &identity, "workouts:read").await;
        let write_token = issue_token(
            &domain,
            &identity,
            "workouts:read workouts:write catalogue:read",
        )
        .await;
        let admin_token = issue_token(
            &domain,
            &identity,
            "workouts:read workouts:write catalogue:read catalogue:write",
        )
        .await;
        let foreign_input: CreateWorkoutSession = serde_json::from_value(json!({
            "started_at": "2026-01-01T00:00:00Z",
            "label": "private",
            "activity": {"type": "strength"}
        }))
        .unwrap();
        let foreign_session_id = domain
            .create_session(
                &test_oauth_principal(other_id, "user", "workouts:read workouts:write"),
                foreign_input,
            )
            .await
            .unwrap()
            .id;
        ProtocolFixture {
            db,
            domain,
            read_token,
            workouts_only_token,
            write_token,
            admin_token,
            foreign_session_id,
        }
    }

    #[tokio::test]
    async fn protected_mcp_blocks_before_rmcp() {
        let (db, domain, token) = mcp_fixture().await;
        let protected = router(domain, CancellationToken::new(), local_config());
        for request in [
            initialize_request(None),
            Request::post("/mcp")
                .header(header::HOST, "127.0.0.1:3000")
                .header(header::COOKIE, "__Host-frater_session=ignored")
                .body(Body::empty())
                .unwrap(),
            initialize_request(Some("Bearer invalid")),
        ] {
            let response = protected.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            let challenge = response.headers()[header::WWW_AUTHENTICATE]
                .to_str()
                .unwrap();
            assert!(challenge.starts_with("Bearer resource_metadata=\"http://127.0.0.1:3000/.well-known/oauth-protected-resource/mcp\""));
        }

        let valid = protected
            .clone()
            .oneshot(initialize_request(Some(&format!("bEaReR {token}"))))
            .await
            .unwrap();
        assert_eq!(valid.status(), StatusCode::OK);

        db.execute_unprepared("UPDATE oauth_access_tokens SET scope='offline_access'")
            .await
            .unwrap();
        let insufficient = protected
            .oneshot(initialize_request(Some(&format!("Bearer {token}"))))
            .await
            .unwrap();
        assert_eq!(insufficient.status(), StatusCode::FORBIDDEN);
        assert!(
            insufficient.headers()[header::WWW_AUTHENTICATE]
                .to_str()
                .unwrap()
                .contains("error=\"insufficient_scope\"")
        );
    }

    #[tokio::test]
    async fn every_response_carries_a_request_id() {
        let (_db, domain, _token) = mcp_fixture().await;
        let app = router(domain, CancellationToken::new(), local_config());
        let send = |request: Request<Body>| {
            let app = app.clone();
            async move { app.oneshot(request).await.unwrap() }
        };

        let generated = send(Request::get("/").body(Body::empty()).unwrap()).await;
        let id = generated.headers()[&request_id::REQUEST_ID_HEADER]
            .to_str()
            .unwrap()
            .to_owned();
        assert_eq!(id.len(), 32);

        let reused = send(
            Request::get("/")
                .header(&request_id::REQUEST_ID_HEADER, "edge-7.abc_123")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(
            reused.headers()[&request_id::REQUEST_ID_HEADER],
            "edge-7.abc_123"
        );
        let replaced = send(
            Request::get("/")
                .header(&request_id::REQUEST_ID_HEADER, "bad id\ttab")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_ne!(
            replaced.headers()[&request_id::REQUEST_ID_HEADER],
            "bad id\ttab"
        );

        let unauthorized = send(
            Request::post("/logout")
                .header(&request_id::REQUEST_ID_HEADER, "api-error")
                .header(
                    axum::http::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded",
                )
                .body(Body::from("csrf=x"))
                .unwrap(),
        )
        .await;
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(body_json(unauthorized).await["request_id"], "api-error");

        let challenged = send(
            Request::post("/mcp")
                .header(header::HOST, "127.0.0.1:3000")
                .header(&request_id::REQUEST_ID_HEADER, "mcp-error")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(challenged.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(body_json(challenged).await["request_id"], "mcp-error");
    }

    async fn body_json(response: Response<Body>) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn text_responses_compress_but_the_font_does_not() {
        let (_db, domain, _token) = mcp_fixture().await;
        let app = router(domain, CancellationToken::new(), local_config());
        let encoding = |path: &'static str| {
            let app = app.clone();
            async move {
                let response = app
                    .oneshot(
                        Request::get(path)
                            .header(header::ACCEPT_ENCODING, "gzip")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                response
                    .headers()
                    .get(header::CONTENT_ENCODING)
                    .map(|value| value.to_str().unwrap().to_owned())
            }
        };
        assert_eq!(encoding("/").await.as_deref(), Some("gzip"));
        assert_eq!(
            encoding(crate::web::views::STYLES_PATH).await.as_deref(),
            Some("gzip")
        );
        assert_eq!(encoding(crate::web::views::FONT_PATH).await, None);
    }

    #[tokio::test]
    async fn protected_mcp_accepts_default_port_origin_and_rejects_other_ports() {
        let (_db, domain, token) = mcp_fixture().await;
        let app = router(domain, CancellationToken::new(), local_config());
        let with_origin = |origin: &'static str| {
            let mut request = initialize_request(Some(&format!("Bearer {token}")));
            request.headers_mut().insert(
                "forwarded",
                HeaderValue::from_static("proto=https;host=frater.example:443"),
            );
            request
                .headers_mut()
                .insert(header::ORIGIN, HeaderValue::from_static(origin));
            request
        };
        let same = app
            .clone()
            .oneshot(with_origin("https://frater.example"))
            .await
            .unwrap();
        assert_ne!(same.status(), StatusCode::FORBIDDEN);
        let different_port = app
            .clone()
            .oneshot(with_origin("https://frater.example:8443"))
            .await
            .unwrap();
        assert_eq!(different_port.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn protected_mcp_actual_protocol_exposes_scoped_tools_and_resources() {
        let fixture = protocol_fixture().await;
        let app = router(
            fixture.domain.clone(),
            CancellationToken::new(),
            local_config(),
        );
        let bearer = |token: &str| format!("Bearer {token}");
        let read = bearer(&fixture.read_token);
        let write = bearer(&fixture.write_token);
        let admin = bearer(&fixture.admin_token);
        let workouts_only = bearer(&fixture.workouts_only_token);

        let initialized = response_json(
            app.clone()
                .oneshot(initialize_request(Some(&read)))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(initialized["result"]["protocolVersion"], "2026-07-28");
        assert!(initialized["result"]["capabilities"].get("tools").is_some());
        assert!(
            initialized["result"]["capabilities"]
                .get("resources")
                .is_some()
        );

        let tools = response_json(
            app.clone()
                .oneshot(mcp_request(
                    Some(&read),
                    json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
                ))
                .await
                .unwrap(),
        )
        .await;
        let actual = tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<std::collections::HashSet<_>>();
        // The token holds workouts:read and catalogue:read, so only the read
        // tools are offered. Every write tool is filtered out of discovery.
        let expected = [
            "list_muscles",
            "get_muscle",
            "list_equipment",
            "get_equipment",
            "list_exercises",
            "get_exercise",
            "list_workout_sessions",
            "get_workout_session",
            "list_session_exercises",
            "get_session_exercise",
            "list_exercise_sets",
            "get_exercise_set",
            "workout_history",
            "exercise_history",
            "personal_records",
            "volume_stats",
        ]
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
        assert_eq!(actual, expected);

        let resources = response_json(
            app.clone()
                .oneshot(mcp_request(
                    Some(&read),
                    json!({"jsonrpc":"2.0","id":3,"method":"resources/list","params":{}}),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            resources["result"]["resources"].as_array().unwrap().len(),
            4
        );
        let templates = response_json(
            app.clone()
                .oneshot(mcp_request(
                    Some(&read),
                    json!({"jsonrpc":"2.0","id":4,"method":"resources/templates/list","params":{}}),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            templates["result"]["resourceTemplates"]
                .as_array()
                .unwrap()
                .len(),
            4
        );

        let denied_write = response_json(
            app.clone()
                .oneshot(mcp_request(
                    Some(&read),
                    json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"create_workout_session","arguments":{"started_at":"2026-01-02T00:00:00Z","activity":{"type":"strength"}}}}),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(denied_write["result"]["isError"], true);
        assert_eq!(
            denied_write["result"]["structuredContent"]["error"],
            "insufficient_scope"
        );
        assert_eq!(
            denied_write["result"]["structuredContent"]["message"],
            "workouts:write scope required"
        );

        let denied_catalogue = response_json(
            app.clone()
                .oneshot(mcp_request(
                    Some(&write),
                    json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"create_muscle","arguments":{"name":"Denied"}}}),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            denied_catalogue["result"]["structuredContent"]["error"],
            "insufficient_scope"
        );
        assert_eq!(
            denied_catalogue["result"]["structuredContent"]["message"],
            "catalogue:write scope required"
        );

        let created_muscle = response_json(
            app.clone()
                .oneshot(mcp_request(
                    Some(&admin),
                    json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"create_muscle","arguments":{"name":"Protocol muscle"}}}),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            created_muscle["result"]["structuredContent"]["name"],
            "Protocol muscle"
        );
        let created_session = response_json(
            app.clone()
                .oneshot(mcp_request(
                    Some(&write),
                    json!({"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"create_workout_session","arguments":{"started_at":"2026-01-02T00:00:00Z","label":"protocol","activity":{"type":"strength"}}}}),
                ))
                .await
                .unwrap(),
        )
        .await;
        let session_id = created_session["result"]["structuredContent"]["id"]
            .as_str()
            .unwrap();

        let list_read = response_json(
            app.clone()
                .oneshot(mcp_request(
                    Some(&read),
                    json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"list_muscles","arguments":{}}}),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            list_read["result"]["structuredContent"]["items"][0]["name"],
            "Protocol muscle"
        );
        let resource_read = response_json(
            app.clone()
                .oneshot(mcp_request(
                    Some(&read),
                    json!({"jsonrpc":"2.0","id":10,"method":"resources/read","params":{"uri":"frater://catalogue/muscles"}}),
                ))
                .await
                .unwrap(),
        )
        .await;
        let resource_text = resource_read["result"]["contents"][0]["text"]
            .as_str()
            .unwrap();
        assert!(resource_text.contains("Protocol muscle"));
        let denied_resource = response_json(
            app.clone()
                .oneshot(mcp_request(
                    Some(&workouts_only),
                    json!({"jsonrpc":"2.0","id":10,"method":"resources/read","params":{"uri":"frater://catalogue/muscles"}}),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert!(
            denied_resource["error"]["message"]
                .as_str()
                .unwrap()
                .contains("catalogue:read scope required")
        );
        let item_read = response_json(
            app.clone()
                .oneshot(mcp_request(
                    Some(&read),
                    json!({"jsonrpc":"2.0","id":11,"method":"resources/read","params":{"uri":format!("frater://workouts/sessions/{session_id}")}}),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert!(
            item_read["result"]["contents"][0]["text"]
                .as_str()
                .unwrap()
                .contains("protocol")
        );

        for (id, uri) in [
            (12, "frater://workouts/sessions/not-a-uuid".to_owned()),
            (
                13,
                format!("frater://workouts/sessions/{}", fixture.foreign_session_id),
            ),
            (
                14,
                "frater://workouts/sessions/00000000-0000-0000-0000-000000000000/extra".to_owned(),
            ),
        ] {
            let response = response_json(
                app.clone()
                    .oneshot(mcp_request(
                        Some(&read),
                        json!({"jsonrpc":"2.0","id":id,"method":"resources/read","params":{"uri":uri}}),
                    ))
                    .await
                    .unwrap(),
            )
            .await;
            assert!(response.get("error").is_some());
            assert!(!response.to_string().contains("private"));
        }
        fixture.db.close().await.unwrap();
    }
}
