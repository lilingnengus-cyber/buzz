use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use life_workbench_contracts::normalized_input_hash;
use life_workbench_mcp::{
    client::LifeClient, config::Config, read_tool_names, registered_tools, validate_tool_call,
    write_tool_names,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tokio::net::TcpListener;
use uuid::Uuid;

#[derive(Default)]
struct MockState {
    consume_requests: Mutex<Vec<Value>>,
    api_requests: Mutex<Vec<Value>>,
    api_attempts: AtomicUsize,
    fail_first_api: AtomicBool,
    oversized_api: AtomicBool,
}

#[tokio::test]
async fn fixed_tools_consume_complete_grant_and_call_only_the_fixed_route() {
    let trace_id = Uuid::new_v4();
    let state = Arc::new(MockState::default());
    let (origin, server) = mock_server(state.clone(), trace_id).await;
    let client = LifeClient::new(config(&origin, trace_id)).expect("client");

    let result = client
        .invoke(
            "list_projects",
            json!({"workspaceId":"workspace-1","limit":25}),
        )
        .await
        .expect("successful read");
    let result: Value = serde_json::from_str(&result).expect("JSON result");
    assert_eq!(result["ok"], true);
    assert_eq!(result["traceId"], trace_id.to_string());

    let consumes = state.consume_requests.lock().expect("consume lock");
    assert_eq!(consumes.len(), 1);
    let consume = &consumes[0];
    assert_eq!(consume["agentId"], "life-agent");
    assert_eq!(consume["agentTurnId"], "turn-1");
    assert_eq!(consume["tool"], "list_projects");
    assert_eq!(consume["capability"], "project:read");
    assert_eq!(
        consume["resource"],
        json!({
            "type":"workspace",
            "id":"workspace-1",
            "expectedVersion":null
        })
    );
    assert_eq!(
        consume["normalizedInputHash"],
        normalized_input_hash(&json!({"archived":false,"limit":25})).expect("hash")
    );
    assert!(Uuid::parse_str(consume["idempotencyKey"].as_str().expect("key")).is_ok());
    assert_eq!(consume["traceId"], trace_id.to_string());

    let api = state.api_requests.lock().expect("API lock");
    assert_eq!(api.len(), 1);
    assert_eq!(api[0]["input"], json!({"archived":false,"limit":25}));
    assert_eq!(api[0]["resource"]["id"], "workspace-1");
    assert!(api[0]["resource"].get("type").is_none());
    assert_eq!(api[0]["idempotencyKey"], consume["idempotencyKey"]);
    drop(api);
    drop(consumes);
    server.abort();
}

#[tokio::test]
async fn temporary_api_failure_retries_only_api_and_oversized_output_is_rejected() {
    let trace_id = Uuid::new_v4();
    let retry_state = Arc::new(MockState::default());
    retry_state.fail_first_api.store(true, Ordering::SeqCst);
    let (origin, retry_server) = mock_server(retry_state.clone(), trace_id).await;
    let client = LifeClient::new(config(&origin, trace_id)).expect("client");
    client
        .invoke("list_projects", json!({"workspaceId":"workspace-1"}))
        .await
        .expect("retry succeeds");
    assert_eq!(retry_state.api_attempts.load(Ordering::SeqCst), 2);
    assert_eq!(
        retry_state
            .consume_requests
            .lock()
            .expect("consume lock")
            .len(),
        1,
        "a temporary API error must not consume the delegation twice",
    );
    retry_server.abort();

    let oversized_state = Arc::new(MockState::default());
    oversized_state.oversized_api.store(true, Ordering::SeqCst);
    let (origin, oversized_server) = mock_server(oversized_state, trace_id).await;
    let client = LifeClient::new(config(&origin, trace_id)).expect("client");
    let error = client
        .invoke("list_projects", json!({"workspaceId":"workspace-1"}))
        .await
        .expect_err("oversized response rejected");
    assert!(matches!(
        error,
        life_workbench_mcp::client::ClientError::InvalidResponse
    ));
    oversized_server.abort();
}

#[tokio::test]
async fn write_transport_failure_is_never_retried_blindly() {
    let trace_id = Uuid::new_v4();
    let state = Arc::new(MockState::default());
    state.fail_first_api.store(true, Ordering::SeqCst);
    let (origin, server) = mock_server(state.clone(), trace_id).await;
    let client = LifeClient::new(config(&origin, trace_id)).expect("client");
    let error = client
        .invoke(
            "update_action_status",
            json!({"actionId":"action-1","expectedVersion":7,"status":"DONE"}),
        )
        .await
        .expect_err("write outcome must remain unknown");
    assert!(matches!(
        error,
        life_workbench_mcp::client::ClientError::WriteOutcomeUnknown
    ));
    assert_eq!(state.api_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(
        state.consume_requests.lock().expect("consume lock").len(),
        1
    );
    server.abort();
}

#[tokio::test]
async fn identical_calls_in_one_turn_use_one_deterministic_idempotency_key() {
    let trace_id = Uuid::new_v4();
    let state = Arc::new(MockState::default());
    let (origin, server) = mock_server(state.clone(), trace_id).await;
    let client = LifeClient::new(config(&origin, trace_id)).expect("client");
    for _ in 0..2 {
        client
            .invoke("list_projects", json!({"workspaceId":"workspace-1"}))
            .await
            .expect("call");
    }
    let requests = state.consume_requests.lock().expect("consume lock");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["idempotencyKey"], requests[1]["idempotencyKey"]);
    drop(requests);
    server.abort();
}

#[tokio::test]
async fn confirmed_write_uses_only_the_gateway_bound_command_resource() {
    let trace_id = Uuid::new_v4();
    let state = Arc::new(MockState::default());
    let (origin, server) = mock_server(state.clone(), trace_id).await;
    let client = LifeClient::new(config(&origin, trace_id)).expect("client");

    client
        .invoke("execute_confirmed_life_write", json!({}))
        .await
        .expect("confirmed write");

    let consumes = state.consume_requests.lock().expect("consume lock");
    assert_eq!(consumes.len(), 1);
    assert_eq!(consumes[0]["capability"], "write_command:execute");
    assert!(consumes[0].get("resource").is_none());
    let api = state.api_requests.lock().expect("API lock");
    assert_eq!(api.len(), 1);
    assert_eq!(api[0]["input"], json!({}));
    assert!(api[0]["resource"].get("type").is_none());
    assert_eq!(
        api[0]["resource"]["id"],
        "018f4d22-8df1-7a67-8ec1-432ad80c615a"
    );
    assert_eq!(api[0]["resource"]["expectedVersion"], 7);
    assert_eq!(api[0]["resource"]["previewHash"], "a".repeat(64));
    drop(api);
    drop(consumes);
    server.abort();
}

#[test]
fn tools_list_is_exact_and_every_schema_is_closed() {
    let expected = BTreeSet::from([
        "get_action_detail",
        "get_ai_execution_context",
        "get_project_context",
        "get_review_context",
        "get_system_overview",
        "get_today_context",
        "get_weekly_review_context",
        "get_knowledge_item",
        "list_actions",
        "list_projects",
        "search_journal",
        "search_knowledge",
    ]);
    let declared = read_tool_names().iter().copied().collect::<BTreeSet<_>>();
    let registered = registered_tools()
        .into_iter()
        .map(|tool| {
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
            assert!(tool["description"]
                .as_str()
                .is_some_and(|value| !value.is_empty()));
            tool["name"].as_str().expect("tool name").to_owned()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(declared, expected);
    let expected_writes = BTreeSet::from([
        "append_ai_execution_output",
        "apply_weekly_review",
        "create_action",
        "create_daily_review",
        "create_goal",
        "create_journal_entry",
        "create_knowledge_item",
        "create_project",
        "create_project_review",
        "finish_ai_execution",
        "preview_life_write",
        "reorder_action_children",
        "set_today_focus",
        "start_ai_execution",
        "update_action",
        "update_action_status",
    ]);
    assert_eq!(
        write_tool_names().iter().copied().collect::<BTreeSet<_>>(),
        expected_writes
    );
    let all_expected = expected
        .into_iter()
        .chain(expected_writes)
        .chain(["execute_confirmed_life_write"])
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert_eq!(registered, all_expected);

    let confirmed = registered_tools()
        .into_iter()
        .find(|tool| tool["name"] == "execute_confirmed_life_write")
        .expect("confirmed tool");
    assert!(confirmed["inputSchema"].get("properties").is_none());
    assert_eq!(confirmed["inputSchema"]["additionalProperties"], false);
}

#[test]
fn arbitrary_url_sql_prisma_where_and_extra_fields_fail_closed() {
    for call in [
        json!({"workspaceId":"workspace-1","url":"https://evil.test"}),
        json!({"workspaceId":"workspace-1","sql":"select * from users"}),
        json!({"workspaceId":"workspace-1","where":{"workspaceId":"other"}}),
        json!({"workspaceId":"workspace-1","extra":true}),
    ] {
        assert!(!validate_tool_call("list_projects", call));
    }
    assert!(!validate_tool_call("run_sql", json!({})));
}

async fn mock_server(
    state: Arc<MockState>,
    trace_id: Uuid,
) -> (String, tokio::task::JoinHandle<()>) {
    async fn consume(
        State((state, trace_id)): State<(Arc<MockState>, Uuid)>,
        headers: HeaderMap,
        Json(request): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        assert_eq!(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some(format!("Bearer {}", "d".repeat(43)).as_str())
        );
        state
            .consume_requests
            .lock()
            .expect("consume lock")
            .push(request.clone());
        let confirmed = request["capability"] == "write_command:execute";
        let resource = if confirmed {
            json!({
                "type":"write_command",
                "id":"018f4d22-8df1-7a67-8ec1-432ad80c615a",
                "expectedVersion":7,
                "previewHash":"a".repeat(64)
            })
        } else {
            request["resource"].clone()
        };
        (
            StatusCode::OK,
            Json(json!({
                "token":"grant-secret-that-must-not-leak",
                "claims": {
                    "capability": request["capability"],
                    "resourceType": resource["type"],
                    "resourceId": resource["id"],
                    "expectedVersion": resource["expectedVersion"],
                    "previewHash": resource.get("previewHash"),
                    "normalizedInputHash": request["normalizedInputHash"],
                    "idempotencyKey": request["idempotencyKey"],
                    "traceId": trace_id
                }
            })),
        )
    }

    async fn projects(
        State((state, trace_id)): State<(Arc<MockState>, Uuid)>,
        headers: HeaderMap,
        Json(request): Json<Value>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse as _;
        assert_eq!(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer grant-secret-that-must-not-leak")
        );
        assert_eq!(
            headers
                .get("x-life-workbench-service-token")
                .and_then(|value| value.to_str().ok()),
            Some("ssssssssssssssssssssssssssssssss")
        );
        state.api_requests.lock().expect("API lock").push(request);
        state.api_attempts.fetch_add(1, Ordering::SeqCst);
        if state.fail_first_api.swap(false, Ordering::SeqCst) {
            return (StatusCode::SERVICE_UNAVAILABLE, "temporary").into_response();
        }
        if state.oversized_api.load(Ordering::SeqCst) {
            return (StatusCode::OK, "x".repeat(300_000)).into_response();
        }
        (
            StatusCode::OK,
            Json(json!({
                "ok":true,
                "data":{"projects":[]},
                "resourceRefs":[],
                "auditId":Uuid::new_v4(),
                "traceId":trace_id
            })),
        )
            .into_response()
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    let app = Router::new()
        .route("/v1/life-agent/delegations/consume", post(consume))
        .route("/api/workbench/projects", post(projects))
        .route("/api/workbench/actions/status", post(projects))
        .route("/api/workbench/write-commands/execute", post(projects))
        .with_state((state, trace_id));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock server");
    });
    (format!("http://{address}"), server)
}

fn config(origin: &str, trace_id: Uuid) -> Config {
    Config::from_values(&BTreeMap::from([
        ("LIFE_DELEGATION_TOKEN".into(), "d".repeat(43)),
        ("LIFE_AUTH_GATEWAY_URL".into(), origin.into()),
        ("LIFE_API_URL".into(), origin.into()),
        ("LIFE_WORKBENCH_MCP_SERVICE_TOKEN".into(), "s".repeat(32)),
        ("LIFE_AGENT_ID".into(), "life-agent".into()),
        ("LIFE_AGENT_TURN_ID".into(), "turn-1".into()),
        ("LIFE_TRACE_ID".into(), trace_id.to_string()),
    ]))
    .expect("config")
}
