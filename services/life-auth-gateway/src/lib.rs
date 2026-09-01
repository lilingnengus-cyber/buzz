//! Isolated authentication and delegation boundary for LifeOS Workbench.

#![deny(missing_docs)]

use axum::{
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::get,
    Router,
};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{sync::Arc, time::Duration};

/// Strong environment configuration.
pub mod config;
/// Wire models introduced as the Gateway gains fixed endpoints.
pub mod model;
/// Secret comparison and Ed25519 key material.
pub mod security;

pub use config::Config;

/// Minimal application state required by liveness and readiness probes.
#[derive(Clone)]
pub struct AppState {
    pool: PgPool,
    signing_key: security::SigningKeyMaterial,
}

impl AppState {
    /// Creates health state from an isolated database pool and signing key.
    pub fn new(pool: PgPool, signing_key: security::SigningKeyMaterial) -> Self {
        Self { pool, signing_key }
    }
}

/// Builds the phase-two skeleton router with no proxy or product endpoints.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .with_state(Arc::new(state))
        .layer(middleware::from_fn(security_headers))
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024))
}

async fn live() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn ready(State(state): State<Arc<AppState>>) -> StatusCode {
    if !state.signing_key.ready() {
        return StatusCode::SERVICE_UNAVAILABLE;
    }
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
    {
        Ok(1) => StatusCode::NO_CONTENT,
        Ok(_) | Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
    );
    response
}

/// Connects the isolated database pool and serves the validated Gateway.
pub async fn run(config: Config) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(Duration::from_secs(2))
        .connect_lazy(config.database_url())?;
    let state = AppState::new(pool, config.signing_key().clone());
    let listener = tokio::net::TcpListener::bind(config.bind_addr()).await?;
    tracing::info!(address = %config.bind_addr(), "life auth gateway listening");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    fn unavailable_state() -> AppState {
        AppState::new(
            PgPoolOptions::new()
                .acquire_timeout(std::time::Duration::from_millis(20))
                .connect_lazy("postgres://life:secret@127.0.0.1:1/life_auth")
                .expect("lazy pool"),
            security::SigningKeyMaterial::parse(&"11".repeat(32)).expect("signing key"),
        )
    }

    #[tokio::test]
    async fn liveness_does_not_query_dependencies() {
        let response = router(unavailable_state())
            .oneshot(
                Request::get("/health/live")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn readiness_fails_when_database_is_unavailable() {
        let response = router(unavailable_state())
            .oneshot(
                Request::get("/health/ready")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn unknown_routes_are_not_proxied() {
        let response = router(unavailable_state())
            .oneshot(
                Request::post("/api/workbench/anything")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }
}
