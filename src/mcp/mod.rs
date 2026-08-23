mod auth;
#[cfg(test)]
mod conformance_tests;
mod resources;
mod schemas;
mod tools;

use std::{sync::Arc, time::Duration};

use axum::http::request::Parts;
use rmcp::{
    handler::server::{
        router::tool::{ToolRoute, ToolRouter},
        tool::ToolCallContext,
    },
    model::{ProtocolVersion, Tool},
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
    },
};
use tokio_util::sync::CancellationToken;

use crate::domain::{Domain, Principal};

use schemas::{TOOL_SPECS, output_schema_for_tool, schema_for_tool};

/// The one protocol revision this server serves. The list bounds what
/// `initialize` may agree to, so every offer gets 2026-07-28 back: per the MCP
/// lifecycle the server answers an unsupported offer with a revision it does
/// support, and the client then decides whether to continue.
pub(crate) const SUPPORTED_PROTOCOL_VERSIONS: &[ProtocolVersion] = &[ProtocolVersion::V_2026_07_28];

#[derive(Clone)]
pub struct McpServer {
    domain: Arc<Domain>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for McpServer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("McpServer").finish_non_exhaustive()
    }
}

impl McpServer {
    fn new(domain: Arc<Domain>) -> Self {
        let mut server = Self {
            domain,
            tool_router: ToolRouter::new(),
        };
        for (name, description, _) in TOOL_SPECS {
            let tool = Tool::new(*name, *description, schema_for_tool(name))
                .with_raw_output_schema(Arc::new(output_schema_for_tool(name)));
            server.tool_router.add_route(ToolRoute::new_dyn(
                tool,
                |mut context: ToolCallContext<'_, Self>| {
                    let name = context.name.into_owned();
                    let arguments = context.arguments.take().unwrap_or_default();
                    let principal = context
                        .request_context
                        .extensions
                        .get::<Parts>()
                        .and_then(|parts| parts.extensions.get::<Principal>())
                        .cloned();
                    let service = context.service;
                    Box::pin(async move {
                        service
                            .dispatch_tool(&name, arguments, principal)
                            .await
                            .map(Into::into)
                    })
                },
            ));
        }
        server
    }
}

/// The protected `/mcp` router: the streamable-http service behind the
/// bearer gate. It carries no timeout layer, because an SSE response is
/// long-lived.
pub fn router(state: crate::app::AppState, cancellation: CancellationToken) -> axum::Router {
    let (allowed_hosts, allowed_origins) = state.origin.mcp_security();
    let service = service(
        state.domain.clone(),
        cancellation,
        allowed_hosts,
        allowed_origins,
    );
    axum::Router::new()
        .nest_service("/mcp", service)
        .route_layer(axum::middleware::from_fn_with_state(
            state,
            auth::require_mcp_oauth,
        ))
}

pub fn service(
    domain: Arc<Domain>,
    cancellation_token: CancellationToken,
    allowed_hosts: Vec<String>,
    allowed_origins: Vec<String>,
) -> StreamableHttpService<McpServer, NeverSessionManager> {
    let config = StreamableHttpServerConfig::default()
        .with_allowed_hosts(allowed_hosts)
        .with_allowed_origins(allowed_origins)
        .with_legacy_session_mode(false)
        .with_stateless_protocol_metadata_required(true)
        .with_json_response(false)
        .with_sse_keep_alive(Some(Duration::from_secs(10)))
        .with_cancellation_token(cancellation_token);

    StreamableHttpService::new(
        move || Ok(McpServer::new(domain.clone())),
        NeverSessionManager::default().into(),
        config,
    )
}

#[cfg(test)]
mod test_support {
    use super::*;
    use crate::{
        domain::{AuthConfig, OAuthConfig, test_oauth_principal},
        migration::Migrator,
    };
    use rmcp::model::CallToolResult;
    use sea_orm::{ConnectionTrait, Database};
    use sea_orm_migration::MigratorTrait;
    use serde_json::Value;
    use uuid::Uuid;

    pub(super) async fn server_fixture() -> (McpServer, Principal, Principal, Principal, Uuid) {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared("PRAGMA foreign_keys=ON")
            .await
            .unwrap();
        Migrator::up(&db, None).await.unwrap();
        let read_id = Uuid::now_v7();
        let admin_id = Uuid::now_v7();
        let super_id = Uuid::now_v7();
        for (id, email, role) in [
            (read_id, "reader@example.com", "user"),
            (admin_id, "admin@example.com", "user"),
            (super_id, "root@example.com", "superuser"),
        ] {
            db.execute_unprepared(&format!(
                "INSERT INTO users(id,email_normalized,email_display,role,status,auth_version,created_at,updated_at) VALUES('{id}','{email}','{email}','{role}','active',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')"
            ))
            .await
            .unwrap();
        }
        let domain = Arc::new(
            Domain::new(
                db,
                AuthConfig {
                    session_hmac_key: [1; 32],
                    session_key_id: "session".into(),
                    password_pepper: b"pepper".to_vec(),
                    pepper_key_id: "pepper".into(),
                    password_concurrency: 1,
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
        (
            McpServer::new(domain),
            test_oauth_principal(read_id, "user", "workouts:read catalogue:read"),
            test_oauth_principal(
                admin_id,
                "user",
                "workouts:read workouts:write catalogue:read catalogue:write",
            ),
            test_oauth_principal(
                super_id,
                "superuser",
                "workouts:read workouts:write catalogue:read catalogue:write",
            ),
            read_id,
        )
    }

    pub(super) fn structured_value(result: &CallToolResult) -> Value {
        serde_json::to_value(result)
            .unwrap()
            .get("structuredContent")
            .cloned()
            .unwrap()
    }
}
