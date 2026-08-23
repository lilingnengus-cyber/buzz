#![forbid(unsafe_code)]

pub mod auth;
pub mod config;
pub mod http;
pub mod model;
pub mod store;

use axum::{
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

pub use config::Config;
pub use http::{router, AppState};
pub use store::Store;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Unauthorized(&'static str),
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Unavailable(&'static str),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Unauthorized(code) => (StatusCode::UNAUTHORIZED, code),
            Self::Forbidden(code) => (StatusCode::FORBIDDEN, code),
            Self::Invalid(code) => (StatusCode::BAD_REQUEST, code),
            Self::Conflict(code) => (StatusCode::CONFLICT, code),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::Unavailable(code) => (StatusCode::SERVICE_UNAVAILABLE, code),
        };
        let mut response = (status, Json(serde_json::json!({"error":code}))).into_response();
        no_store(response.headers_mut());
        response
    }
}

pub(crate) fn no_store(headers: &mut HeaderMap) {
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    headers.insert(
        axum::http::header::PRAGMA,
        HeaderValue::from_static("no-cache"),
    );
    headers.insert(
        axum::http::header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
}
