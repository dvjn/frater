use axum::{
    extract::Request,
    http::{HeaderMap, HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;
use uuid::Uuid;

pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

#[derive(Clone, Debug)]
pub struct RequestId(Arc<str>);

impl RequestId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

tokio::task_local! {
    static CURRENT: RequestId;
}

pub fn current() -> Option<String> {
    CURRENT.try_with(|id| id.as_str().to_owned()).ok()
}

pub async fn propagate(mut request: Request, next: Next) -> Response {
    let id = supplied(request.headers()).unwrap_or_else(new_id);
    let header = HeaderValue::from_str(&id).ok();
    let id = RequestId(Arc::from(id.as_str()));
    request.extensions_mut().insert(id.clone());
    let mut response = CURRENT.scope(id, next.run(request)).await;
    if let Some(header) = header {
        response.headers_mut().insert(REQUEST_ID_HEADER, header);
    }
    response
}

fn new_id() -> String {
    Uuid::now_v7().simple().to_string()
}

/// A reverse proxy supplies the id, so the value is reused to correlate the
/// proxy log with the application log. The value is bounded and restricted to
/// safe characters, because it reaches a log line and a response header.
fn supplied(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(&REQUEST_ID_HEADER)?.to_str().ok()?;
    let acceptable = (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    acceptable.then(|| value.to_owned())
}
