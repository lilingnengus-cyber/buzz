use super::*;

pub(super) async fn check(store: &Store, pool: &sqlx::PgPool, keys: &Keys, human: Uuid) {
    let scope = "operational_adjustment_post_intent:approve";
    let permission: Uuid = sqlx::query_scalar("INSERT INTO business_iam.principal_permissions(principal_id,permission_id) SELECT $1,id FROM business_iam.permissions WHERE capability=$2 RETURNING permission_id")
        .bind(human).bind(scope).fetch_one(pool).await.unwrap();
    for mode in [
        "stale", "future", "tampered", "channel", "family", "suffix", "bare", "disabled",
    ] {
        let channel = Uuid::new_v4().to_string();
        let id = Uuid::new_v4();
        let hash = "d".repeat(64);
        let content = match mode {
            "bare" => "确认".to_owned(),
            "suffix" => format!("确认 operational-adjustment-post-intent {id} v1 {hash} extra"),
            "family" => format!("确认 operating-report-snapshot-intent {id} v1 {hash}"),
            _ => format!("确认 operational-adjustment-post-intent {id} v1 {hash}"),
        };
        let mut builder = EventBuilder::new(Kind::TextNote, content)
            .tags([Tag::custom(TagKind::Custom("h".into()), [channel.clone()])]);
        if matches!(mode, "stale" | "future") {
            let timestamp = Utc::now().timestamp() + if mode == "stale" { -600 } else { 600 };
            builder = builder.custom_created_at(nostr::Timestamp::from_secs(timestamp as u64));
        }
        let mut event = builder.sign_with_keys(keys).unwrap();
        if mode == "tampered" {
            event.content = format!("拒绝 operational-adjustment-post-intent {id} v1 {hash}");
        }
        let before: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_read_delegations")
            .fetch_one(pool)
            .await
            .unwrap();
        let mut disabled_config = config(String::new(), 4);
        disabled_config.business_chat_approval_enabled = false;
        let disabled = Store::new(pool.clone(), disabled_config);
        let target = if mode == "disabled" { &disabled } else { store };
        let result = target
            .issue_agent_delegation(
                IssueAgentDelegationRequest {
                    source_buzz_event_id: event.id.to_hex(),
                    source_buzz_pubkey: event.pubkey.to_hex(),
                    source_event: event,
                    source_channel_id: if mode == "channel" {
                        Uuid::new_v4().to_string()
                    } else {
                        channel
                    },
                    agent_id: "business-query-agent".into(),
                    agent_turn_id: format!("adjustment-negative-{mode}"),
                    scopes: vec![scope.into()],
                },
                facts(Uuid::new_v4()),
            )
            .await;
        assert!(
            matches!(result, Err(Rejection::Forbidden("agent_turn_rejected"))),
            "{mode}"
        );
        let after: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_read_delegations")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(before, after, "{mode} must not mint a delegation");
    }
    sqlx::query(
        "DELETE FROM business_iam.principal_permissions WHERE principal_id=$1 AND permission_id=$2",
    )
    .bind(human)
    .bind(permission)
    .execute(pool)
    .await
    .unwrap();
}
