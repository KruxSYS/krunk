//! Shared pieces of the JSON API (PROTOCOL §11.1): the error type, a strict JSON body extractor
//! and the trusted client of a request.

use crate::client_ip::{self, ClientInfo};
use crate::http::AppState;
use axum::extract::{ConnectInfo, FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// `{"error":"<code>","message":"<generic text>"}`, plus `Retry-After` for rate limits.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: &'static str,
    pub retry_after: Option<u64>,
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
    message: &'static str,
}

impl ApiError {
    pub const fn new(status: StatusCode, code: &'static str, message: &'static str) -> ApiError {
        ApiError { status, code, message, retry_after: None }
    }

    pub fn bad_request(code: &'static str, message: &'static str) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, code, message)
    }

    pub fn unauthorized() -> ApiError {
        ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "The email address or password is wrong.")
    }

    pub fn not_signed_in() -> ApiError {
        ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "Please sign in.")
    }

    pub fn forbidden() -> ApiError {
        ApiError::new(StatusCode::FORBIDDEN, "forbidden", "This request is not allowed.")
    }

    pub fn rate_limited(retry_after: u64) -> ApiError {
        ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "rate_limited",
            message: "Too many attempts. Please wait and try again.",
            retry_after: Some(retry_after.max(1)),
        }
    }

    pub fn server_error() -> ApiError {
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "Something went wrong. Please try again.")
    }

    pub fn accounts_disabled() -> ApiError {
        ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "accounts_disabled", "Accounts are coming soon.")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut res = (self.status, Json(ErrorBody { error: self.code, message: self.message })).into_response();
        if let Some(s) = self.retry_after {
            res.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from(s));
        }
        res
    }
}

/// Database errors are logged (without parameters) and answered with a generic 500.
impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> ApiError {
        tracing::error!(error = %e, "database error");
        ApiError::server_error()
    }
}

pub type ApiResult<T = Response> = Result<T, ApiError>;

/// A JSON body: `application/json`, valid, and matching the type (whose fields use
/// `deny_unknown_fields`). Anything else is a JSON `400 bad_request`, never axum's plain text.
pub struct JsonBody<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for JsonBody<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(JsonBody(v)),
            Err(rejection) => {
                if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                    return Err(ApiError::new(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "payload_too_large",
                        "The request is too large.",
                    ));
                }
                Err(ApiError::bad_request("bad_request", "The request is not valid."))
            }
        }
    }
}

/// The trusted client of the request (PLAN §4.4) and its user agent (≤ 256 characters).
pub struct Client {
    pub info: ClientInfo,
    pub user_agent: Option<String>,
}

impl Client {
    pub fn ip(&self) -> IpAddr {
        self.info.ip
    }
}

pub fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

impl FromRequestParts<AppState> for Client {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip())
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        let info = client_ip::client_info(peer, &parts.headers, &state.trusted);
        let user_agent = parts
            .headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(|s| truncate_chars(s.trim(), 256))
            .filter(|s| !s.is_empty());
        Ok(Client { info, user_agent })
    }
}
