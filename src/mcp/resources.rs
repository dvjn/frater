use std::borrow::Cow;

use axum::http::request::Parts;
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CacheScope, Implementation, ListResourceTemplatesResult, ListResourcesResult,
        PaginatedRequestParams, ProtocolVersion, ReadResourceRequestParams, ReadResourceResponse,
        ReadResourceResult, Resource, ResourceContents, ResourceTemplate, ServerCapabilities,
        ServerInfo,
    },
    service::RequestContext,
    tool_handler,
};
use serde_json::json;
use uuid::Uuid;

use crate::domain::{DomainError, PageRequest, Principal, SessionFilter};

use super::{McpServer, SUPPORTED_PROTOCOL_VERSIONS};

const JSON_MIME: &str = "application/json";
const COLLECTION_HELP: &str = "This stable resource returns the first bounded page (50 items). Use the corresponding list_* tool with offset/limit and filters for later pages.";

#[tool_handler(router = self.tool_router)]
impl ServerHandler for McpServer {
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(SUPPORTED_PROTOCOL_VERSIONS)
    }

    fn get_info(&self) -> ServerInfo {
        let capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .build();
        let instructions = "Authenticated fitness catalogue and owner-scoped workout CRUD. Catalogue reads require catalogue:read; catalogue writes require catalogue:write plus superuser; workout reads require workouts:read; workout writes require workouts:write.";
        ServerInfo::new(capabilities)
            .with_protocol_version(ProtocolVersion::V_2026_07_28)
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(instructions)
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        require_resource_principal(&context, &["workouts:read", "catalogue:read"])?;
        Ok(private_resources(vec![
            resource(
                "frater://catalogue/muscles",
                "muscles",
                "Muscle catalogue (first page)",
            ),
            resource(
                "frater://catalogue/equipment",
                "equipment",
                "Equipment catalogue (first page)",
            ),
            resource(
                "frater://catalogue/exercises",
                "exercises",
                "Exercise catalogue summaries (first page; use item resources for associations)",
            ),
            resource(
                "frater://workouts/sessions",
                "workout_sessions",
                "Your workout session summaries (first page)",
            ),
        ]))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        require_resource_principal(&context, &["workouts:read", "catalogue:read"])?;
        Ok(private_templates(vec![
            template(
                "frater://catalogue/muscles/{id}",
                "muscle",
                "A muscle catalogue item",
            ),
            template(
                "frater://catalogue/equipment/{id}",
                "equipment_item",
                "An equipment catalogue item",
            ),
            template(
                "frater://catalogue/exercises/{id}",
                "exercise",
                "An exercise and its muscle/equipment associations",
            ),
            template(
                "frater://workouts/sessions/{id}",
                "workout_session",
                "One owned workout session with its complete hierarchy",
            ),
        ]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let uri = request.uri;
        let principal = require_resource_principal(&context, &[resource_scope(&uri)])?;
        let value = match uri.as_str() {
            "frater://catalogue/muscles" => {
                let page = self
                    .domain
                    .list_muscles(None, PageRequest::default())
                    .await
                    .map_err(resource_domain_error)?;
                json!({"page": page, "pagination_help": COLLECTION_HELP})
            }
            "frater://catalogue/equipment" => {
                let page = self
                    .domain
                    .list_equipment(None, PageRequest::default())
                    .await
                    .map_err(resource_domain_error)?;
                json!({"page": page, "pagination_help": COLLECTION_HELP})
            }
            "frater://catalogue/exercises" => {
                let page = self
                    .domain
                    .list_exercises(None, PageRequest::default())
                    .await
                    .map_err(resource_domain_error)?;
                json!({"page": page, "pagination_help": COLLECTION_HELP})
            }
            "frater://workouts/sessions" => {
                let page = self
                    .domain
                    .list_sessions(&principal, SessionFilter::default(), PageRequest::default())
                    .await
                    .map_err(resource_domain_error)?;
                json!({"page": page, "pagination_help": COLLECTION_HELP})
            }
            _ => {
                let (prefix, id) = uri
                    .rsplit_once('/')
                    .ok_or_else(|| ErrorData::resource_not_found("resource not found", None))?;
                let id = Uuid::parse_str(id)
                    .map_err(|_| ErrorData::resource_not_found("resource not found", None))?;
                match prefix {
                    "frater://catalogue/muscles" => serde_json::to_value(
                        self.domain
                            .get_muscle(id)
                            .await
                            .map_err(resource_domain_error)?,
                    ),
                    "frater://catalogue/equipment" => serde_json::to_value(
                        self.domain
                            .get_equipment(id)
                            .await
                            .map_err(resource_domain_error)?,
                    ),
                    "frater://catalogue/exercises" => serde_json::to_value(
                        self.domain
                            .get_exercise(id)
                            .await
                            .map_err(resource_domain_error)?,
                    ),
                    "frater://workouts/sessions" => serde_json::to_value(
                        self.domain
                            .get_session(&principal, id)
                            .await
                            .map_err(resource_domain_error)?,
                    ),
                    _ => return Err(ErrorData::resource_not_found("resource not found", None)),
                }
                .map_err(|_| ErrorData::internal_error("could not serialize resource", None))?
            }
        };
        let text = serde_json::to_string_pretty(&value)
            .map_err(|_| ErrorData::internal_error("could not serialize resource", None))?;
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(text, uri).with_mime_type(JSON_MIME),
        ])
        .with_ttl_ms(0)
        .with_cache_scope(CacheScope::Private)
        .into())
    }
}

fn resource_scope(uri: &str) -> &'static str {
    if uri.starts_with("frater://catalogue/") {
        "catalogue:read"
    } else {
        "workouts:read"
    }
}

fn require_resource_principal(
    context: &RequestContext<RoleServer>,
    required: &[&str],
) -> Result<Principal, ErrorData> {
    let principal = context
        .extensions
        .get::<Parts>()
        .and_then(|parts| parts.extensions.get::<Principal>())
        .cloned()
        .ok_or_else(|| {
            ErrorData::invalid_request("authenticated OAuth principal required", None)
        })?;
    if principal
        .oauth()
        .is_some_and(|oauth| required.iter().any(|required| oauth.has_scope(required)))
    {
        Ok(principal)
    } else {
        Err(ErrorData::invalid_request(
            format!("{} scope required", required.join(" or ")),
            None,
        ))
    }
}

fn resource_domain_error(error: DomainError) -> ErrorData {
    match error {
        DomainError::NotFound => ErrorData::resource_not_found("resource not found", None),
        DomainError::InvalidInput(message) => ErrorData::invalid_params(message, None),
        DomainError::Forbidden | DomainError::InvalidCredentials => {
            ErrorData::invalid_request("forbidden", None)
        }
        DomainError::Conflict => ErrorData::invalid_request("conflict", None),
        error @ (DomainError::TemporarilyUnavailable | DomainError::Internal(_)) => {
            tracing::error!(?error, "resource read failed");
            ErrorData::internal_error("fitness service unavailable", None)
        }
    }
}

fn resource(uri: &str, name: &str, description: &str) -> Resource {
    Resource::new(uri, name)
        .with_description(format!("{description}. {COLLECTION_HELP}"))
        .with_mime_type(JSON_MIME)
}

fn template(uri: &str, name: &str, description: &str) -> ResourceTemplate {
    ResourceTemplate::new(uri, name)
        .with_description(description)
        .with_mime_type(JSON_MIME)
}

fn private_resources(resources: Vec<Resource>) -> ListResourcesResult {
    ListResourcesResult::with_all_items(resources)
        .with_ttl_ms(0)
        .with_cache_scope(CacheScope::Private)
}

fn private_templates(resource_templates: Vec<ResourceTemplate>) -> ListResourceTemplatesResult {
    ListResourceTemplatesResult::with_all_items(resource_templates)
        .with_ttl_ms(0)
        .with_cache_scope(CacheScope::Private)
}
