//! Isolated authentication and delegation boundary for LifeOS Workbench.

#![deny(missing_docs)]

use axum::{
    extract::Request,
    http::{header, HeaderValue},
    middleware::{self, Next},
    response::Response,
    Router,
};
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

/// Signed-source Agent turn delegations and atomic call consumption.
pub mod agent;
/// Low-sensitivity security-audit vocabulary.
pub mod audit;
/// OIDC validation for Workbench access tokens.
pub mod auth;
/// Ed25519 LifeCallGrant construction.
pub mod call_grant;
/// Versioned LifeOS capability and fixed-tool catalog.
pub mod catalog;
/// Strong environment configuration.
pub mod config;
/// Fail-closed LifeOS channel-disclosure policy lookups.
pub mod disclosure;
/// One-time Life Dock bootstrap codes and Embed Sessions.
pub mod embed;
/// Fixed HTTP surface for Life identity and health operations.
pub mod http;
/// Transactional authorization over current Life identities and authority.
pub mod iam;
/// Explicit LifeOS identity resolution and Nostr binding workflows.
pub mod identity;
/// Monotonic LifeOS membership snapshot ingestion.
pub mod membership;
/// Low-cardinality authorization metrics.
pub mod metrics;
/// Wire models introduced as the Gateway gains fixed endpoints.
pub mod model;
/// Secret comparison and Ed25519 key material.
pub mod security;
/// Transactional persistence over the isolated Life security schema.
pub mod store;
/// Short-lived, single-use Pacioli target-selection tickets.
pub mod target_selection;
/// Exact signed confirmation grants for immutable LifeOS WriteCommands.
pub mod write_confirmation;

pub use config::Config;
pub use http::AppState;
pub use store::Store;

/// Builds the fixed Life authentication router with no generic proxy surface.
pub fn router(state: AppState) -> Router {
    http::router(state)
        .layer(middleware::from_fn(security_headers))
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024))
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
        .connect(config.database_url())
        .await?;
    Store::migrate(&pool).await?;
    catalog::validate_persisted(&pool).await?;
    let state = AppState::configured(pool, &config)?;
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
