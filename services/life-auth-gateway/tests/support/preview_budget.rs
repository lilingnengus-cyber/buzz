use super::*;
use life_auth_gateway::agent::AgentError;

#[tokio::test]
async fn lookups_reserve_preview_and_concurrent_writes_close_the_turn() {
    let Some(database) = TestDatabase::create().await else {
        eprintln!("LIFE_AUTH_TEST_DATABASE_URL absent; preview test skipped");
        return;
    };
    let keys = Keys::generate();
    let store = seed_authority(&database.pool, &keys).await;
    let channel = Uuid::new_v4();
    let mut request = issue_request(source(&keys, channel), channel, "preview-turn");
    request.requested_capabilities = vec!["action:read".into(), "write_command:preview".into()];
    request.resource_context = None;
    let issued = store
        .issue_agent_delegation(request, &policy(), &current_identity())
        .await
        .expect("delegation");
    assert_eq!(issued.max_calls, 4);
    let signer = CallGrantSigner::new(
        "life-auth-test",
        "lifeos-workbench-api",
        Duration::from_secs(30),
        SigningKeyMaterial::parse(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let call = |read: bool| ConsumeDelegationRequest {
        agent_id: "life-agent".into(),
        agent_turn_id: "preview-turn".into(),
        tool: if read {
            "get_action_detail"
        } else {
            "preview_life_write"
        }
        .into(),
        capability: if read {
            "action:read"
        } else {
            "write_command:preview"
        }
        .into(),
        resource: Some(ResourceContext {
            resource_type: "action".into(),
            id: "action-1".into(),
            expected_version: if read { None } else { Some(7) },
            preview_hash: None,
        }),
        normalized_input_hash: format!("sha256:{}", "b".repeat(64)),
        idempotency_key: Uuid::new_v4().to_string(),
        trace_id: issued.trace_id,
    };
    for _ in 0..3 {
        store
            .consume_agent_delegation(&issued.token, call(true), &signer)
            .await
            .expect("lookup");
    }
    assert!(matches!(
        store
            .consume_agent_delegation(&issued.token, call(true), &signer)
            .await,
        Err(AgentError::RateLimited)
    ));
    let (a, b) = tokio::join!(
        store.consume_agent_delegation(&issued.token, call(false), &signer),
        store.consume_agent_delegation(&issued.token, call(false), &signer)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(matches!(a, Err(AgentError::RateLimited)) || matches!(b, Err(AgentError::RateLimited)));
    assert!(matches!(
        store
            .consume_agent_delegation(&issued.token, call(true), &signer)
            .await,
        Err(AgentError::RateLimited)
    ));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM life_delegation_calls WHERE delegation_id=$1 AND capability='write_command:preview'")
        .bind(issued.delegation_id.as_uuid()).fetch_one(&database.pool).await.unwrap();
    assert_eq!(count, 1);
    database.cleanup().await;
}
