//! Request access logs.
//!
//! Each router gets its own access log layer, and each layer writes under its
//! own tracing target. The targets form a hierarchy, and `EnvFilter` obeys the
//! most specific directive. Thus `RUST_LOG` alone can turn one class of request
//! logs on or off, and no code change or extra configuration variable is
//! necessary:
//!
//! - `frater=info` — every log, request logs included
//! - `frater=info,frater::access_log::healthz=off` — all except the healthcheck
//! - `frater::access_log=off` — no request log, the other logs unchanged
//! - `frater=warn,frater::access_log::oauth=info` — the OAuth requests only
//!
//! The subsystems are [`AUTH`], [`OAUTH`], [`MCP`], [`ASSETS`] and [`HEALTHZ`].
//! No layer writes under the parent target `frater::access_log` itself. It
//! exists so that one directive can control every subsystem below it. The
//! targets stay under the crate path, so one `frater` directive still covers the
//! whole binary.
//!
//! A route must appear in exactly one of these routers. Two layers over the
//! same route write two lines for one request, and one directive then cannot
//! silence both. To add a subsystem: add a target constant here, give the
//! subsystem its own router, and apply one layer to it in [`crate::app`].

/// The browser pages: sign in, registration, password reset, the account pages.
pub const AUTH: &str = "frater::access_log::auth";

/// The OAuth surface: metadata, registration, device flow, authorize, token.
pub const OAUTH: &str = "frater::access_log::oauth";

/// The MCP transport.
pub const MCP: &str = "frater::access_log::mcp";

/// The content-addressed assets: the stylesheet and the font.
pub const ASSETS: &str = "frater::access_log::assets";

/// The healthcheck. A container platform or an uptime probe calls `/healthz`
/// continuously, so operators often want these lines off while the other
/// request logs stay on.
pub const HEALTHZ: &str = "frater::access_log::healthz";

/// The filter that applies when `RUST_LOG` is absent. The assets and the
/// healthcheck are noisy and carry little information, so they start off.
pub const DEFAULT_FILTER: &str =
    "frater=info,frater::access_log::assets=off,frater::access_log::healthz=off";

/// The filter for the subscriber. It reads `RUST_LOG`, and it falls back to
/// [`DEFAULT_FILTER`]. An empty or invalid `RUST_LOG` also falls back, because a
/// server that writes no log at all is never the intent.
pub fn filter() -> tracing_subscriber::EnvFilter {
    match std::env::var("RUST_LOG") {
        Ok(value) if !value.trim().is_empty() => tracing_subscriber::EnvFilter::try_new(&value)
            .unwrap_or_else(|error| {
                eprintln!("RUST_LOG is invalid ({error}); using {DEFAULT_FILTER}");
                tracing_subscriber::EnvFilter::new(DEFAULT_FILTER)
            }),
        _ => tracing_subscriber::EnvFilter::new(DEFAULT_FILTER),
    }
}

/// Builds one access log layer for the given target.
///
/// This is a macro, not a function, because a tracing target must be known when
/// the event macro expands.
macro_rules! layer {
    ($target:expr) => {
        tower_http::trace::TraceLayer::new_for_http()
            // Headers, cookies and the query string can carry credentials,
            // authorization codes and tokens. Thus the span records only the
            // method, the path and the protocol version.
            .make_span_with(|request: &axum::http::Request<_>| {
                tracing::info_span!(
                    target: $target,
                    "request",
                    method = %request.method(),
                    path = request.uri().path(),
                    version = ?request.version(),
                    request_id = request
                        .extensions()
                        .get::<$crate::request_id::RequestId>()
                        .map($crate::request_id::RequestId::as_str)
                        .unwrap_or_default(),
                )
            })
            .on_request(())
            .on_response(
                |response: &axum::response::Response<_>,
                 latency: std::time::Duration,
                 _span: &tracing::Span| {
                    tracing::info!(
                        target: $target,
                        status = response.status().as_u16(),
                        latency_ms = latency.as_millis() as u64,
                        "request completed"
                    );
                },
            )
            // The default handlers write under a `tower_http` target, which an
            // access log directive could not control. The response event above
            // already records the status, so the failure handler only adds the
            // classification, and the body handlers add nothing.
            .on_failure(
                |failure: tower_http::classify::ServerErrorsFailureClass,
                 latency: std::time::Duration,
                 _span: &tracing::Span| {
                    tracing::warn!(
                        target: $target,
                        %failure,
                        latency_ms = latency.as_millis() as u64,
                        "request failed"
                    );
                },
            )
            .on_body_chunk(())
            .on_eos(())
    };
}

pub(crate) use layer;
