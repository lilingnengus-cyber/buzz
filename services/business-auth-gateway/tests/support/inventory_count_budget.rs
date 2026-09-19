use super::*;

pub(super) async fn check(pool: &sqlx::PgPool, keys: &Keys, user: Uuid, binding: Uuid) {
    let mut cfg = config(std::env::var("TEST_DATABASE_URL").unwrap(), 64);
    cfg.agent_delegation_ttl = Duration::from_secs(900);
    let store = Store::new(pool.clone(), cfg);
    let trace = Uuid::new_v4();
    let channel = Uuid::new_v4().to_string();
    let event = EventBuilder::new(Kind::TextNote, "盘点分页调用预算验收")
        .tags([Tag::custom(TagKind::Custom("h".into()), [channel.clone()])])
        .sign_with_keys(keys)
        .unwrap();
    let request = IssueAgentDelegationRequest {
        source_event: event.clone(),
        source_buzz_event_id: event.id.to_hex(),
        source_buzz_pubkey: event.pubkey.to_hex(),
        source_channel_id: channel,
        agent_id: "business-query-agent".into(),
        agent_turn_id: "count-paging-budget".into(),
        scopes: vec![
            "inventory:read".into(),
            "inventory_count_submission_intent:create".into(),
        ],
    };
    let issued = store
        .issue_agent_delegation(request, facts(trace))
        .await
        .unwrap();
    assert_eq!(issued.max_calls, 64);
    let consume = |tool: &str, scope: &str| ConsumeAgentDelegationRequest {
        tool_name: tool.into(),
        required_scope: scope.into(),
        agent_id: "business-query-agent".into(),
        agent_turn_id: "count-paging-budget".into(),
    };
    // No approval grant can be obtained by increasing the ordinary call budget.
    assert!(store
        .consume_agent_delegation(
            &issued.token,
            consume(
                "approve_inventory_count_submission",
                "inventory_count_submission_intent:approve"
            ),
            facts(trace)
        )
        .await
        .is_err());
    // 25 detail pages, one preparation, and 24 additional preview pages.
    for call in 0..50 {
        let (tool, scope) = match call {
            0..=24 => ("get_inventory_count", "inventory:read"),
            25 => (
                "prepare_inventory_count_submission",
                "inventory_count_submission_intent:create",
            ),
            _ => ("get_inventory_count_approval_preview", "inventory:read"),
        };
        let consumed = store
            .consume_agent_delegation(&issued.token, consume(tool, scope), facts(trace))
            .await
            .unwrap();
        assert_eq!(consumed.used_calls, call + 1);
        let verified = store
            .verify_agent_delegation(
                VerifyAgentDelegationRequest {
                    delegation_id: issued.id,
                    enterprise_user_id: user,
                    identity_binding_id: binding,
                    agent_id: "business-query-agent".into(),
                    agent_turn_id: "count-paging-budget".into(),
                    trace_id: trace,
                    used_calls: call + 1,
                    required_scope: scope.into(),
                    approval: None,
                },
                facts(trace),
            )
            .await
            .unwrap();
        assert_eq!(verified.capability.as_str(), scope);
    }
    // Concurrent consumers cannot overdraw the remaining fourteen calls.
    let mut tasks = Vec::new();
    for _ in 0..20 {
        let store = store.clone();
        let token = issued.token.clone();
        let request = consume("get_inventory_count_approval_preview", "inventory:read");
        tasks.push(tokio::spawn(async move {
            store
                .consume_agent_delegation(&token, request, facts(trace))
                .await
                .is_ok()
        }));
    }
    let mut accepted = 0;
    for task in tasks {
        accepted += usize::from(task.await.unwrap());
    }
    assert_eq!(accepted, 14);
    for _ in 0..2 {
        assert!(store
            .consume_agent_delegation(
                &issued.token,
                consume("get_inventory_count_approval_preview", "inventory:read"),
                facts(trace)
            )
            .await
            .is_err());
    }
    let row = sqlx::query(
        "SELECT used_calls,max_calls,status,expires_at FROM agent_read_delegations WHERE id=$1",
    )
    .bind(issued.id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(row.get::<i32, _>("used_calls"), 64);
    assert_eq!(row.get::<i32, _>("max_calls"), 64);
    assert_eq!(row.get::<String, _>("status"), "exhausted");
    assert_eq!(
        row.get::<chrono::DateTime<Utc>, _>("expires_at")
            .timestamp(),
        issued.expires_at.timestamp()
    );
}
