//! The HTTP surface: routes, JSON errors, response headers, limits and request logs.

use crate::client_ip::{self, TrustedPeers};
use crate::config::Config;
use axum::extract::{ConnectInfo, DefaultBodyLimit, MatchedPath, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use sqlx::PgPool;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

/// Default request body limit (PROTOCOL §11.1). Larger limits are set per route when needed.
pub const BODY_LIMIT: usize = 16 * 1024;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub trusted: TrustedPeers,
}

/// `{"error":"<code>","message":"<generic text>"}` (PROTOCOL §11.1).
#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
    message: &'static str,
}

pub fn error(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (status, Json(ErrorBody { error: code, message })).into_response()
}

pub fn router(state: AppState) -> Router {
    let api = Router::new().route("/api/v1/web/config", get(web_config)).fallback(not_found);
    Router::new()
        .route("/healthz", get(healthz))
        .merge(api)
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(RequestBodyLimitLayer::new(BODY_LIMIT))
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, REQUEST_TIMEOUT))
        .layer(middleware::from_fn_with_state(state.clone(), log_request))
        .layer(middleware::from_fn(response_headers))
        .with_state(state)
}

/// Every response: never cached anywhere, no content sniffing. A bare 413 from the body
/// limit becomes the usual JSON error.
async fn response_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let is_json =
        res.headers().get(header::CONTENT_TYPE).is_some_and(|v| v.as_bytes().starts_with(b"application/json"));
    if res.status() == StatusCode::PAYLOAD_TOO_LARGE && !is_json {
        res = error(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", "The request is too large.");
    }
    let h = res.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    res
}

/// One log line per request: method, route template, status, latency and the masked client IP.
/// Never the body, the query string, cookies or other headers.
async fn log_request(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let start = Instant::now();
    let method = req.method().clone();
    let route =
        req.extensions().get::<MatchedPath>().map(|p| p.as_str().to_string()).unwrap_or_else(|| "<unmatched>".into());
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    let client = peer.map(|p| client_ip::client_info(p, req.headers(), &state.trusted));
    let res = next.run(req).await;
    tracing::info!(
        method = %method,
        route = %route,
        status = res.status().as_u16(),
        ms = start.elapsed().as_millis() as u64,
        client = %client.as_ref().map(|c| client_ip::masked(c.ip)).unwrap_or_else(|| "-".into()),
        country = %client.and_then(|c| c.country).unwrap_or_else(|| "-".into()),
        "request"
    );
    res
}

async fn not_found() -> Response {
    error(StatusCode::NOT_FOUND, "not_found", "Not found.")
}

/// Internal health check (not proxied publicly): the process runs and the database answers.
async fn healthz(State(state): State<AppState>) -> Response {
    // A health check must answer fast, also when the database hangs.
    let probe = sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&state.db);
    match tokio::time::timeout(HEALTH_TIMEOUT, probe).await {
        Ok(Ok(1)) => (StatusCode::OK, "ok").into_response(),
        _ => error(StatusCode::SERVICE_UNAVAILABLE, "server_error", "Database unavailable."),
    }
}

#[derive(Serialize)]
struct WebConfig<'a> {
    turnstile_site_key: &'a str,
    public_base_url: &'a str,
}

/// `GET /api/v1/web/config` (PROTOCOL §11.2): public configuration for the static pages.
async fn web_config(State(state): State<AppState>) -> Response {
    Json(WebConfig {
        turnstile_site_key: &state.config.turnstile_site_key,
        public_base_url: &state.config.public_base_url,
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;

    fn state() -> AppState {
        // A lazy pool never connects unless a handler queries it.
        let db = sqlx::postgres::PgPoolOptions::new().connect_lazy("postgres://x:y@127.0.0.1:1/none").unwrap();
        AppState {
            config: Config {
                database_url: "postgres://x:y@127.0.0.1:1/none".into(),
                bind: "127.0.0.1:0".parse().unwrap(),
                public_base_url: "https://krunk.209.wf".into(),
                turnstile_site_key: "1x00000000000000000000AA".into(),
                trusted_peer_host: "web".into(),
            },
            db,
            trusted: TrustedPeers::default(),
        }
    }

    async fn call(req: axum::http::Request<Body>) -> (StatusCode, axum::http::HeaderMap, String) {
        let res = router(state()).oneshot(req).await.unwrap();
        let (parts, body) = res.into_parts();
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        (parts.status, parts.headers, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn web_config_is_public_and_never_cached() {
        let (status, h, body) = call(axum::http::Request::get("/api/v1/web/config").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(h[header::CACHE_CONTROL], "no-store");
        assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["turnstile_site_key"], "1x00000000000000000000AA");
        assert_eq!(v["public_base_url"], "https://krunk.209.wf");
    }

    #[tokio::test]
    async fn unknown_paths_are_json_404() {
        for path in ["/api/v1/nope", "/", "/api/v1/web/config/x"] {
            let (status, h, body) = call(axum::http::Request::get(path).body(Body::empty()).unwrap()).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
            assert_eq!(h[header::CACHE_CONTROL], "no-store");
            assert_eq!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["error"], "not_found");
        }
    }

    #[tokio::test]
    async fn oversized_bodies_are_refused() {
        let big = vec![b'a'; BODY_LIMIT + 1];
        let req = axum::http::Request::post("/api/v1/web/config")
            .header(header::CONTENT_LENGTH, big.len())
            .body(Body::from(big))
            .unwrap();
        let (status, h, body) = call(req).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(h[header::CACHE_CONTROL], "no-store");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["error"], "payload_too_large");
    }

    #[tokio::test]
    async fn health_reports_an_unreachable_database() {
        let (status, h, _) = call(axum::http::Request::get("/healthz").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(h[header::CACHE_CONTROL], "no-store");
    }
}
