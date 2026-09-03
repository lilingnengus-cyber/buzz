use axum::{routing::post, Json, Router};
use chrono::{Duration, Utc};
use life_auth_gateway::{
    disclosure::{DisclosureCategory, DisclosureClient, DisclosureError, DisclosureSensitivity},
    security::OutboundServiceCredential,
};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use url::Url;
use uuid::Uuid;

async fn server(response: Value) -> (Url, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    let app = Router::new().route(
        "/api/internal/pacioli-disclosure/evaluate",
        post(move || async move { Json(response) }),
    );
    let handle = tokio::spawn(async move { axum::serve(listener, app).await.expect("server") });
    (
        Url::parse(&format!("http://{address}/")).expect("URL"),
        handle,
    )
}

fn credential() -> OutboundServiceCredential {
    OutboundServiceCredential::parse("TEST", "l".repeat(32)).expect("credential")
}

#[tokio::test]
async fn valid_policy_requires_all_minimization_obligations() {
    let (base, handle) = server(json!({
        "ok":true,
        "allowed":true,
        "policyId":Uuid::new_v4(),
        "expiresAt":(Utc::now() + Duration::minutes(5)).to_rfc3339(),
        "obligations":["read_only","redact_sensitive","summary_only"],
        "reason":null
    }))
    .await;
    let grant = DisclosureClient::new(&base, &credential())
        .expect("client")
        .evaluate(
            "life-user",
            "community",
            "channel",
            DisclosureCategory::ActionSummary,
            DisclosureSensitivity::Normal,
        )
        .await
        .expect("grant");
    assert!(grant.allowed);
    assert!(DisclosureCategory::ActionSummary
        .capabilities()
        .iter()
        .all(|capability| capability.ends_with(":read")));
    handle.abort();
}

#[tokio::test]
async fn missing_obligation_and_policy_outage_fail_closed() {
    let (base, handle) = server(json!({
        "ok":true,
        "allowed":true,
        "policyId":Uuid::new_v4(),
        "expiresAt":(Utc::now() + Duration::minutes(5)).to_rfc3339(),
        "obligations":["read_only"],
        "reason":null
    }))
    .await;
    let client = DisclosureClient::new(&base, &credential()).expect("client");
    assert!(matches!(
        client
            .evaluate(
                "u",
                "c",
                "h",
                DisclosureCategory::ProjectStatus,
                DisclosureSensitivity::Public
            )
            .await,
        Err(DisclosureError::Invalid)
    ));
    handle.abort();
    assert!(matches!(
        client
            .evaluate(
                "u",
                "c",
                "h",
                DisclosureCategory::ProjectStatus,
                DisclosureSensitivity::Public
            )
            .await,
        Err(DisclosureError::Unavailable)
    ));
}
