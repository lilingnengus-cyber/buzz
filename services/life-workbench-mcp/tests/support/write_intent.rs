use super::*;

fn action() -> Value {
    json!({"workspaceId":"workspace-1","projectId":"project-1",
        "title":"验证会话直写", "priority":"HIGH", "focusDate":"2026-09-05"})
}

#[tokio::test]
async fn create_and_focus_forwards_user_key_and_concurrent_duplicates_use_one_call() {
    let trace = Uuid::new_v4();
    let state = Arc::new(MockState::default());
    let (origin, server) = mock_server(state.clone(), trace).await;
    let client = LifeClient::new(config(&origin, trace)).expect("client");
    let key = Uuid::new_v4();
    let mut input = action();
    input["idempotencyKey"] = json!(key);
    let (first, second) = tokio::join!(
        client.invoke("create_action", input.clone()),
        client.invoke("create_action", input.clone())
    );
    assert_eq!(first.expect("created"), second.expect("cached"));
    input["title"] = json!("different action");
    assert!(client.invoke("create_action", input).await.is_err());
    let consumes = state.consume_requests.lock().expect("lock");
    let api = state.api_requests.lock().expect("lock");
    assert_eq!(consumes.len(), 1);
    assert_eq!(api.len(), 1);
    assert_eq!(consumes[0]["idempotencyKey"], json!(key));
    assert_eq!(api[0]["idempotencyKey"], json!(key));
    assert_eq!(api[0]["input"]["value"]["focusDate"], "2026-09-05");
    assert_eq!(api[0]["input"]["value"]["priority"], "HIGH");
    assert_eq!(api[0]["input"]["value"]["projectId"], "project-1");
    assert!(api[0]["input"]["value"].get("idempotencyKey").is_none());
    server.abort();
}

#[tokio::test]
async fn invalid_extraction_never_consumes_authority_and_can_be_corrected() {
    let trace = Uuid::new_v4();
    let state = Arc::new(MockState::default());
    let (origin, server) = mock_server(state.clone(), trace).await;
    let client = LifeClient::new(config(&origin, trace)).expect("client");
    for (field, value) in [
        ("title", "  "),
        ("projectId", ""),
        ("focusDate", "today"),
        ("focusDate", "2026-02-30"),
        ("priority", "highest"),
        ("idempotencyKey", "not-a-uuid"),
    ] {
        let mut input = action();
        input[field] = json!(value);
        assert!(client.invoke("create_action", input).await.is_err());
    }
    assert!(state.consume_requests.lock().expect("lock").is_empty());
    client
        .invoke("create_action", action())
        .await
        .expect("corrected input");
    server.abort();
}

#[tokio::test]
async fn unknown_write_is_cached_and_never_reissued() {
    let trace = Uuid::new_v4();
    let state = Arc::new(MockState::default());
    state.fail_first_api.store(true, Ordering::SeqCst);
    let (origin, server) = mock_server(state.clone(), trace).await;
    let client = LifeClient::new(config(&origin, trace)).expect("client");
    for _ in 0..2 {
        assert!(matches!(
            client.invoke("create_action", action()).await,
            Err(life_workbench_mcp::client::ClientError::WriteOutcomeUnknown)
        ));
    }
    assert_eq!(state.api_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(state.consume_requests.lock().expect("lock").len(), 1);
    server.abort();
}

#[tokio::test]
async fn timeout_and_cancellation_keep_the_write_reserved() {
    for cancel in [false, true] {
        let trace = Uuid::new_v4();
        let state = Arc::new(MockState::default());
        state.slow_api.store(true, Ordering::SeqCst);
        let (origin, server) = mock_server(state.clone(), trace).await;
        let client = LifeClient::new(config(&origin, trace)).expect("client");
        if cancel {
            let write = client.invoke("create_action", action());
            tokio::pin!(write);
            tokio::select! {
                result = &mut write => panic!("unexpected completion: {result:?}"),
                _ = async {
                    while state.api_attempts.load(Ordering::SeqCst) == 0 {
                        tokio::task::yield_now().await;
                    }
                } => {}
            }
            // The pinned future is dropped at the end of this scope.
        } else {
            assert!(matches!(
                client.invoke("create_action", action()).await,
                Err(life_workbench_mcp::client::ClientError::WriteOutcomeUnknown)
            ));
        }
        assert!(matches!(
            client.invoke("create_action", action()).await,
            Err(life_workbench_mcp::client::ClientError::WriteOutcomeUnknown)
        ));
        assert_eq!(state.api_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(state.consume_requests.lock().expect("lock").len(), 1);
        server.abort();
    }
}

#[tokio::test]
async fn separate_agents_do_not_share_keys_or_cached_results() {
    let trace = Uuid::new_v4();
    let state = Arc::new(MockState::default());
    let (origin, server) = mock_server(state.clone(), trace).await;
    // Build distinct per-agent configurations, including the same turn ID.
    for agent in ["proxy-a", "proxy-b"] {
        let config = Config::from_values(&BTreeMap::from([
            ("LIFE_DELEGATION_TOKEN".into(), "d".repeat(43)),
            ("LIFE_AUTH_GATEWAY_URL".into(), origin.clone()),
            ("LIFE_API_URL".into(), origin.clone()),
            ("LIFE_WORKBENCH_MCP_SERVICE_TOKEN".into(), "s".repeat(32)),
            ("LIFE_AGENT_ID".into(), agent.into()),
            ("LIFE_AGENT_TURN_ID".into(), "same-turn".into()),
            ("LIFE_TRACE_ID".into(), trace.to_string()),
        ]))
        .expect("config");
        LifeClient::new(config)
            .expect("client")
            .invoke("create_action", action())
            .await
            .expect("write");
    }
    let requests = state.consume_requests.lock().expect("lock");
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0]["idempotencyKey"], requests[1]["idempotencyKey"]);
    assert_eq!(state.api_attempts.load(Ordering::SeqCst), 2);
    server.abort();
}

#[tokio::test]
async fn a_changed_resource_version_is_a_different_write() {
    let trace = Uuid::new_v4();
    let state = Arc::new(MockState::default());
    let (origin, server) = mock_server(state.clone(), trace).await;
    let client = LifeClient::new(config(&origin, trace)).expect("client");
    client
        .invoke(
            "update_action_status",
            json!({
                "actionId":"action-1","expectedVersion":1,"status":"DONE"
            }),
        )
        .await
        .expect("first write");
    assert!(matches!(
        client
            .invoke(
                "update_action_status",
                json!({
                    "actionId":"action-1","expectedVersion":2,"status":"DONE"
                })
            )
            .await,
        Err(life_workbench_mcp::client::ClientError::RateLimited)
    ));
    assert_eq!(state.api_attempts.load(Ordering::SeqCst), 1);
    server.abort();
}

#[tokio::test]
async fn unreadable_write_response_remains_unknown() {
    let trace = Uuid::new_v4();
    let state = Arc::new(MockState::default());
    state.oversized_api.store(true, Ordering::SeqCst);
    let (origin, server) = mock_server(state.clone(), trace).await;
    let client = LifeClient::new(config(&origin, trace)).expect("client");
    for _ in 0..2 {
        assert!(matches!(
            client.invoke("create_action", action()).await,
            Err(life_workbench_mcp::client::ClientError::WriteOutcomeUnknown)
        ));
    }
    assert_eq!(state.api_attempts.load(Ordering::SeqCst), 1);
    server.abort();
}
