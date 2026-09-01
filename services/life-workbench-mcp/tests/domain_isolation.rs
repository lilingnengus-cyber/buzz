use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use life_workbench_mcp::{client::LifeClient, config::Config};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::net::TcpListener;
use uuid::Uuid;

#[tokio::test]
async fn business_delegation_cannot_cross_the_life_gateway_or_reach_lifeos() {
    #[derive(Default)]
    struct MockState {
        gateway_attempts: AtomicUsize,
        life_api_attempts: AtomicUsize,
    }

    async fn reject_business(
        State(state): State<Arc<MockState>>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> StatusCode {
        state.gateway_attempts.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some(format!("Bearer {}", "b".repeat(43)).as_str())
        );
        assert_eq!(body["capability"], "focus:read");
        StatusCode::UNAUTHORIZED
    }

    async fn life_api(State(state): State<Arc<MockState>>) -> (StatusCode, Json<Value>) {
        state.life_api_attempts.fetch_add(1, Ordering::SeqCst);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({})))
    }

    let state = Arc::new(MockState::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let origin = format!("http://{}", listener.local_addr().expect("address"));
    let app = Router::new()
        .route("/v1/life-agent/delegations/consume", post(reject_business))
        .route("/api/workbench/context/today", post(life_api))
        .with_state(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock boundary");
    });

    let trace_id = Uuid::new_v4();
    let config = Config::from_values(&BTreeMap::from([
        ("LIFE_DELEGATION_TOKEN".into(), "b".repeat(43)),
        ("LIFE_AUTH_GATEWAY_URL".into(), origin.clone()),
        ("LIFE_API_URL".into(), origin),
        ("LIFE_WORKBENCH_MCP_SERVICE_TOKEN".into(), "s".repeat(32)),
        ("LIFE_AGENT_ID".into(), "life-agent".into()),
        ("LIFE_AGENT_TURN_ID".into(), "business-turn".into()),
        ("LIFE_TRACE_ID".into(), trace_id.to_string()),
    ]))
    .expect("config");
    let client = LifeClient::new(config).expect("client");
    let result = client
        .invoke("get_today_context", json!({"workspaceId":"workspace-1"}))
        .await;

    assert!(matches!(
        result,
        Err(life_workbench_mcp::client::ClientError::ScopeDenied)
    ));
    assert_eq!(state.gateway_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(
        state.life_api_attempts.load(Ordering::SeqCst),
        0,
        "a foreign delegation must fail before a LifeOS request"
    );
    server.abort();
}
