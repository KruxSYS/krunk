//! The HTTP surface: routes, JSON errors, response headers, limits and request logs.

use crate::accounts;
use crate::api::{ApiResult, Client};
use crate::approvals;
use crate::client_ip::{self, TrustedPeers};
use crate::config::Config;
use crate::devices;
use crate::keys;
use crate::password::Passwords;
use crate::ratelimit;
use crate::sessions;
use crate::turnstile::Turnstile;
use axum::extract::{ConnectInfo, DefaultBodyLimit, MatchedPath, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use sqlx::PgPool;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

/// Default request body limit (PROTOCOL §11.1). Larger limits are set per route group.
pub const BODY_LIMIT: usize = 16 * 1024;
/// `keys/commit` (two directories and the escrow).
pub const KEYS_BODY_LIMIT: usize = 512 * 1024;
/// The ceiling for every request (`records/batch` and `rekey/batch`, S7, are the largest).
pub const MAX_BODY_LIMIT: usize = 2 * 1024 * 1024;

/// A route group's body limit. A declared `Content-Length` above it is refused with 413 after
/// the body has been read and dropped (up to the ceiling): answering before reading would make
/// the proxy in front (Caddy) see a broken connection and report a 502 instead. A body without
/// a length is cut off at the limit by the extractors (`DefaultBodyLimit`).
fn limited<S: Clone + Send + Sync + 'static>(r: Router<S>, max: usize) -> Router<S> {
    r.layer(DefaultBodyLimit::max(max)).layer(middleware::from_fn_with_state(max, refuse_large))
}

async fn refuse_large(State(max): State<usize>, req: Request, next: Next) -> Response {
    let declared =
        req.headers().get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<usize>().ok());
    match declared {
        Some(n) if n > max => {
            let _ = axum::body::to_bytes(req.into_body(), MAX_BODY_LIMIT).await;
            error(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", "The request is too large.")
        }
        _ => next.run(req).await,
    }
}
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: PgPool,
    pub trusted: TrustedPeers,
    pub passwords: Passwords,
    pub turnstile: Turnstile,
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
    // The account endpoints: closed until Turnstile and SMTP are configured, and state-changing
    // requests only from the site's own origin.
    let web = Router::new()
        .route("/api/v1/web/register", post(accounts::register))
        .route("/api/v1/web/verify-email", post(accounts::verify_email))
        .route("/api/v1/web/resend-verification", post(accounts::resend))
        .route("/api/v1/web/login", post(accounts::login))
        .route("/api/v1/web/logout", post(accounts::logout))
        .route("/api/v1/web/password-reset/request", post(accounts::reset_request))
        .route("/api/v1/web/password-reset/confirm", post(accounts::reset_confirm))
        .route("/api/v1/web/password/change", post(accounts::password_change))
        .route("/api/v1/web/reauth", post(accounts::reauth))
        .route("/api/v1/web/me", get(accounts::me))
        .route("/api/v1/web/totp/setup", post(accounts::totp_setup))
        .route("/api/v1/web/totp/enable", post(accounts::totp_enable))
        .route("/api/v1/web/totp/disable", post(accounts::totp_disable))
        // S3: link approval, devices and sessions.
        .route("/api/v1/web/link/{code}", get(devices::link_view))
        .route("/api/v1/web/link/{code}/approve", post(devices::link_approve))
        .route("/api/v1/web/link/{code}/deny", post(devices::link_deny))
        .route("/api/v1/web/devices", get(devices::devices_list))
        .route("/api/v1/web/devices/{id}/revoke", post(devices::device_revoke))
        .route("/api/v1/web/devices/{id}/rename", post(devices::device_rename))
        .route("/api/v1/web/sessions", get(devices::sessions_list))
        .route("/api/v1/web/sessions/revoke-all", post(devices::sessions_revoke_all))
        .route("/api/v1/web/sessions/{id}/revoke", post(devices::session_revoke))
        .route_layer(middleware::from_fn_with_state(state.clone(), sessions::same_origin))
        .route_layer(middleware::from_fn_with_state(state.clone(), sessions::accounts_gate));
    // S3: native clients (Krunk). No Origin, no cookies: device codes and bearer tokens.
    let device = Router::new()
        .route("/api/v1/link/authorize", post(devices::authorize))
        .route("/api/v1/link/token", post(devices::token))
        .route("/api/v1/token/refresh", post(devices::refresh))
        .route("/api/v1/device/unlink", post(devices::unlink))
        .route("/api/v1/device/me", get(devices::device_me))
        // S6: keys, approvals, sync state, and devices and sessions seen from a device.
        .route("/api/v1/sync/state", get(keys::sync_state))
        .route("/api/v1/keys/head", get(keys::keys_head))
        .route("/api/v1/keys/chain", get(keys::keys_chain))
        .route("/api/v1/keys/escrow", get(keys::keys_escrow))
        .route("/api/v1/keys/rotation/claim", post(keys::rotation_claim))
        .route("/api/v1/approvals", post(approvals::create).get(approvals::list))
        .route("/api/v1/approvals/{id}", get(approvals::get_one))
        .route("/api/v1/approvals/{id}/respond", post(approvals::respond))
        .route("/api/v1/approvals/{id}/reveal", post(approvals::reveal))
        .route("/api/v1/approvals/{id}/confirm", post(approvals::confirm))
        .route("/api/v1/approvals/{id}/keys", post(approvals::keys))
        .route("/api/v1/approvals/{id}/cancel", post(approvals::cancel))
        .route("/api/v1/devices", get(keys::device_devices))
        .route("/api/v1/devices/{id}/revoke", post(keys::device_revoke))
        .route("/api/v1/sessions", get(keys::device_sessions))
        .route("/api/v1/sessions/{id}/revoke", post(keys::device_session_revoke))
        .route_layer(middleware::from_fn_with_state(state.clone(), devices::no_foreign_origin))
        .route_layer(middleware::from_fn_with_state(state.clone(), sessions::accounts_gate));
    // Larger bodies, only where the protocol needs them (PROTOCOL §11.1).
    let keys_commit = Router::new()
        .route("/api/v1/keys/commit", post(keys::keys_commit))
        .route_layer(middleware::from_fn_with_state(state.clone(), devices::no_foreign_origin))
        .route_layer(middleware::from_fn_with_state(state.clone(), sessions::accounts_gate));
    let api = Router::new()
        .route("/api/v1/web/config", get(web_config))
        .merge(limited(web, BODY_LIMIT))
        .merge(limited(device, BODY_LIMIT))
        .merge(limited(keys_commit, KEYS_BODY_LIMIT))
        .fallback(not_found);
    Router::new()
        .route("/healthz", get(healthz))
        .merge(api)
        .fallback(not_found)
        .layer(RequestBodyLimitLayer::new(MAX_BODY_LIMIT))
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
    accounts_enabled: bool,
    turnstile_site_key: &'a str,
    public_base_url: &'a str,
}

/// `GET /api/v1/web/config` (PROTOCOL §11.2): public configuration for the static pages.
async fn web_config(State(state): State<AppState>, client: Client) -> ApiResult {
    // Fails open on a database error: this is public, static information.
    let key = ratelimit::ip_key(&state.config.keys, client.ip());
    if let Err(e) = ratelimit::hit(&state.db, ratelimit::CONFIG_IP, &key).await {
        if e.code == "rate_limited" {
            return Err(e);
        }
    }
    let enabled = state.config.accounts_enabled();
    Ok(Json(WebConfig {
        accounts_enabled: enabled,
        // The widget key goes out as soon as it is configured, before SMTP opens the accounts
        // (0.6.0): the forms show the bot check while registration still answers 503, so the
        // operator can see the provisioning on the live site step by step.
        turnstile_site_key: &state.config.turnstile_site_key,
        public_base_url: &state.config.public_base_url,
    })
    .into_response())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use axum::body::Body;
    use std::sync::OnceLock;
    use tower::ServiceExt;

    /// The Argon2 dummy hash takes a moment; share one hasher between tests.
    pub fn passwords() -> Passwords {
        static P: OnceLock<Passwords> = OnceLock::new();
        P.get_or_init(|| Passwords::new([5u8; 32]).unwrap()).clone()
    }

    pub fn state_with(extra: &[(&str, &str)]) -> AppState {
        let mut env: Vec<(String, String)> = vec![
            ("DATABASE_URL".into(), "postgres://x:y@127.0.0.1:1/none".into()),
            ("PUBLIC_BASE_URL".into(), "https://krunk.209.wf".into()),
        ];
        env.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        let config = Config::from_lookup(&|k| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())).unwrap();
        // A lazy pool never connects unless a handler queries it; then it fails fast.
        let db = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(500))
            .connect_lazy(&config.database_url)
            .unwrap();
        let turnstile = Turnstile::with_url("http://127.0.0.1:9/siteverify", "s", "krunk.209.wf", false).unwrap();
        AppState { config: Arc::new(config), db, trusted: TrustedPeers::default(), passwords: passwords(), turnstile }
    }

    pub const ENABLED: [(&str, &str); 3] = [
        ("TURNSTILE_SITE_KEY", "0x4AAAsite"),
        ("TURNSTILE_SECRET_KEY", "0x4AAAsecret"),
        ("SMTP_HOST", "smtp.example.org"),
    ];

    async fn call_with(state: AppState, req: axum::http::Request<Body>) -> (StatusCode, axum::http::HeaderMap, String) {
        let res = router(state).oneshot(req).await.unwrap();
        let (parts, body) = res.into_parts();
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        (parts.status, parts.headers, String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn call(req: axum::http::Request<Body>) -> (StatusCode, axum::http::HeaderMap, String) {
        call_with(state_with(&[]), req).await
    }

    fn json(body: &str) -> serde_json::Value {
        serde_json::from_str(body).unwrap()
    }

    #[tokio::test]
    async fn web_config_is_public_and_never_cached() {
        // Closed (no Turnstile/SMTP): the (empty) configured site key. The rate limit fails open
        // when the database is unreachable, so the config still answers.
        let (status, h, body) = call(axum::http::Request::get("/api/v1/web/config").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(h[header::CACHE_CONTROL], "no-store");
        assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        let v = json(&body);
        assert_eq!(v["accounts_enabled"], false);
        assert_eq!(v["turnstile_site_key"], "");
        assert_eq!(v["public_base_url"], "https://krunk.209.wf");
        let (_, _, body) = call_with(
            state_with(&ENABLED),
            axum::http::Request::get("/api/v1/web/config").body(Body::empty()).unwrap(),
        )
        .await;
        let v = json(&body);
        assert_eq!(v["accounts_enabled"], true);
        assert_eq!(v["turnstile_site_key"], "0x4AAAsite");
        // Turnstile provisioned but no SMTP yet (0.6.0 M17 steps 1-2): the key goes out, the
        // accounts stay closed.
        let keys_only = [("TURNSTILE_SITE_KEY", "0x4AAAsite"), ("TURNSTILE_SECRET_KEY", "0x4AAAsecret")];
        let (_, _, body) = call_with(
            state_with(&keys_only),
            axum::http::Request::get("/api/v1/web/config").body(Body::empty()).unwrap(),
        )
        .await;
        let v = json(&body);
        assert_eq!(v["accounts_enabled"], false);
        assert_eq!(v["turnstile_site_key"], "0x4AAAsite");
    }

    #[tokio::test]
    async fn accounts_fail_closed() {
        for path in ["/api/v1/web/register", "/api/v1/web/login", "/api/v1/web/password-reset/request"] {
            let req = axum::http::Request::post(path)
                .header(header::ORIGIN, "https://krunk.209.wf")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap();
            let (status, h, body) = call(req).await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{path}");
            assert_eq!(json(&body)["error"], "accounts_disabled");
            assert_eq!(h[header::CACHE_CONTROL], "no-store");
        }
        let (status, _, body) = call(axum::http::Request::get("/api/v1/web/me").body(Body::empty()).unwrap()).await;
        assert_eq!(
            (status, json(&body)["error"].as_str()),
            (StatusCode::SERVICE_UNAVAILABLE, Some("accounts_disabled"))
        );
    }

    #[tokio::test]
    async fn cross_origin_posts_are_refused() {
        let post = |origin: Option<&str>, fetch_site: Option<&str>| {
            let mut b = axum::http::Request::post("/api/v1/web/login").header(header::CONTENT_TYPE, "application/json");
            if let Some(o) = origin {
                b = b.header(header::ORIGIN, o);
            }
            if let Some(f) = fetch_site {
                b = b.header("sec-fetch-site", f);
            }
            b.body(Body::from("{}")).unwrap()
        };
        for (origin, site) in [
            (None, None),
            (Some("https://evil.example"), None),
            (Some("https://krunk.209.wf.evil.example"), Some("same-origin")),
            (Some("null"), None),
            (Some("http://krunk.209.wf"), None),
            (None, Some("cross-site")),
            (None, Some("same-site")),
        ] {
            let (status, _, body) = call_with(state_with(&ENABLED), post(origin, site)).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{origin:?} {site:?}");
            assert_eq!(json(&body)["error"], "forbidden");
        }
        // Same origin passes the check (and then fails on the empty body, before any database use).
        for (origin, site) in [(Some("https://krunk.209.wf"), None), (None, Some("same-origin"))] {
            let (status, _, _) = call_with(state_with(&ENABLED), post(origin, site)).await;
            assert_ne!(status, StatusCode::FORBIDDEN, "{origin:?} {site:?}");
        }
    }

    #[tokio::test]
    async fn signed_in_endpoints_need_the_cookie_and_csrf() {
        let s = state_with(&ENABLED);
        let (status, _, body) =
            call_with(s.clone(), axum::http::Request::get("/api/v1/web/me").body(Body::empty()).unwrap()).await;
        assert_eq!((status, json(&body)["error"].as_str()), (StatusCode::UNAUTHORIZED, Some("unauthorized")));
        // A cookie but no CSRF header on a POST: refused before the database is asked.
        let token = crate::crypto::new_token();
        let req = axum::http::Request::post("/api/v1/web/logout")
            .header(header::ORIGIN, "https://krunk.209.wf")
            .header(header::COOKIE, format!("{}={token}", sessions::COOKIE))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let (status, _, body) = call_with(s.clone(), req).await;
        assert_eq!((status, json(&body)["error"].as_str()), (StatusCode::FORBIDDEN, Some("forbidden")));
        let req = axum::http::Request::post("/api/v1/web/logout")
            .header(header::ORIGIN, "https://krunk.209.wf")
            .header(header::COOKIE, format!("{}={token}", sessions::COOKIE))
            .header(sessions::CSRF_HEADER, sessions::csrf_for(&s.config.keys, &crate::crypto::new_token()))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let (status, _, _) = call_with(s, req).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "a CSRF token of another session");
    }

    #[tokio::test]
    async fn bad_json_is_a_json_400() {
        for body in ["not json", "{\"email\":1}", "{\"email\":\"a@b.cd\",\"turnstile\":\"t\",\"extra\":1}"] {
            let req = axum::http::Request::post("/api/v1/web/password-reset/request")
                .header(header::ORIGIN, "https://krunk.209.wf")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap();
            let (status, h, resp) = call_with(state_with(&ENABLED), req).await;
            // The per-IP limit is checked first and needs the database; without one this is a 500.
            // What matters here: never axum's plain-text rejection.
            assert!(h[header::CONTENT_TYPE].to_str().unwrap().starts_with("application/json"), "{body}: {status}");
            assert!(json(&resp)["error"].is_string());
        }
    }

    #[tokio::test]
    async fn unknown_paths_are_json_404() {
        for path in ["/api/v1/nope", "/", "/api/v1/web/config/x"] {
            let (status, h, body) = call(axum::http::Request::get(path).body(Body::empty()).unwrap()).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
            assert_eq!(h[header::CACHE_CONTROL], "no-store");
            assert_eq!(json(&body)["error"], "not_found");
        }
    }

    #[tokio::test]
    async fn oversized_bodies_are_refused() {
        // Per route group (PROTOCOL §11.1): 16 KiB for the website and device calls, 512 KiB for
        // keys/commit, and a 2 MiB ceiling for everything.
        let post = |path: &str, n: usize| {
            axum::http::Request::post(path)
                .header(header::CONTENT_LENGTH, n)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(vec![b'a'; n]))
                .unwrap()
        };
        for (path, n, too_big) in [
            ("/api/v1/web/login", BODY_LIMIT + 1, true),
            ("/api/v1/approvals", BODY_LIMIT + 1, true),
            ("/api/v1/keys/commit", BODY_LIMIT + 1, false),
            ("/api/v1/keys/commit", 300 * 1024, false),
            ("/api/v1/keys/commit", KEYS_BODY_LIMIT + 1, true),
            ("/api/v1/nope", MAX_BODY_LIMIT + 1, true),
        ] {
            let (status, h, body) = call(post(path, n)).await;
            if too_big {
                assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{path} {n}");
                assert_eq!(h[header::CACHE_CONTROL], "no-store");
                assert_eq!(json(&body)["error"], "payload_too_large");
            } else {
                assert_ne!(status, StatusCode::PAYLOAD_TOO_LARGE, "{path} {n}");
            }
        }
    }

    #[tokio::test]
    async fn health_reports_an_unreachable_database() {
        let (status, h, _) = call(axum::http::Request::get("/healthz").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(h[header::CACHE_CONTROL], "no-store");
    }
}
