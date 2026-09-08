use life_auth_gateway::pending_delete::{PendingDeleteRequest, ShortDeleteRequest};
use nostr::Tag;

#[tokio::test]
async fn short_delete_requires_published_scoped_preview_and_is_single_use() {
    let Some(database) = Database::create().await else {
        assert!(
            std::env::var("LIFE_AUTH_TEST_DATABASE_URL").is_err(),
            "database setup failed"
        );
        return;
    };
    let owner = Keys::generate();
    let agent = Keys::generate();
    let (user, session) = seed(&database.pool, &owner).await;
    let store = Store::new(database.pool.clone());
    let channel = Uuid::new_v4().to_string();
    let delegation = Uuid::new_v4();
    let source = "1".repeat(64);
    sqlx::query("INSERT INTO life_agent_delegations
        (id,token_hash,workbench_user_id,workbench_session_id,agent_id,agent_turn_id,source_event_id,source_pubkey,
         source_channel_id,audience,capabilities,data_scope,obligations,status,expires_at,max_calls,remaining_calls,trace_id)
        VALUES($1,$2,$3,$4,$5,'preview-turn',$6,$7,$8,'life-workbench-mcp','[\"write_command:preview\"]','{}','[]',
         'exhausted',now()+interval '10 minutes',4,0,$9)")
        .bind(delegation).bind(vec![3u8;32]).bind(user.as_uuid()).bind(session.as_uuid())
        .bind(agent.public_key().to_hex()).bind(&source).bind(owner.public_key().to_hex())
        .bind(&channel).bind(Uuid::new_v4()).execute(&database.pool).await.expect("delegation");
    sqlx::query("INSERT INTO life_delegation_calls
        (id,delegation_id,call_id,capability,normalized_input_hash,idempotency_key,expected_version,status,trace_id)
        VALUES($1,$2,$3,'write_command:preview',$4,'preview',7,'issued',$5)")
        .bind(Uuid::new_v4()).bind(delegation).bind(Uuid::new_v4()).bind(vec![4u8;32])
        .bind(Uuid::new_v4()).execute(&database.pool).await.expect("preview call");
    let preview = EventBuilder::new(Kind::Custom(9), "确认删除行动 test 吗？")
        .tags([
            Tag::parse(vec!["h", &channel]).expect("h"),
            Tag::parse(vec!["e", &source]).expect("e"),
        ])
        .custom_created_at(Timestamp::from(Timestamp::now().as_secs() - 2))
        .sign_with_keys(&agent)
        .expect("preview");
    let command_id = Uuid::new_v4();
    store
        .record_pending_delete(PendingDeleteRequest {
            delegation_id: delegation,
            community_id: "community".into(),
            preview_event: preview,
            command_id,
            expected_version: 7,
            preview_hash: "a".repeat(64),
            expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
        })
        .await
        .expect("record published preview");
    let signed = |keys: &Keys, channel: &str, text: &str| {
        EventBuilder::new(Kind::Custom(9), text)
            .tag(Tag::parse(vec!["h", channel]).expect("h"))
            .sign_with_keys(keys)
            .expect("confirmation")
    };
    let request = ShortDeleteRequest {
        signed_event: signed(&owner, &channel, "确认删除"),
        community_id: "community".into(),
        agent_id: agent.public_key().to_hex(),
        trace_id: Uuid::new_v4(),
    };
    let other_thread = EventBuilder::new(Kind::Custom(9), "确认删除")
        .tags([
            Tag::parse(vec!["h", &channel]).expect("h"),
            Tag::parse(vec!["e", &"2".repeat(64), "", "root"]).expect("root"),
        ])
        .sign_with_keys(&owner)
        .expect("other thread");
    assert!(store
        .validate_short_delete(
            ShortDeleteRequest {
                signed_event: other_thread,
                ..request.clone()
            },
            "life-test"
        )
        .await
        .is_err());
    let early = EventBuilder::new(Kind::Custom(9), "确认删除")
        .tag(Tag::parse(vec!["h", &channel]).expect("h"))
        .custom_created_at(Timestamp::from(Timestamp::now().as_secs() - 30))
        .sign_with_keys(&owner)
        .expect("early signed confirmation");
    assert!(store
        .validate_short_delete(
            ShortDeleteRequest {
                signed_event: early,
                ..request.clone()
            },
            "life-test"
        )
        .await
        .is_err());
    for invalid in [
        ShortDeleteRequest {
            community_id: "other".into(),
            ..request.clone()
        },
        ShortDeleteRequest {
            agent_id: Keys::generate().public_key().to_hex(),
            ..request.clone()
        },
        ShortDeleteRequest {
            signed_event: signed(&owner, "other", "确认删除"),
            ..request.clone()
        },
        ShortDeleteRequest {
            signed_event: signed(&Keys::generate(), &channel, "确认删除"),
            ..request.clone()
        },
        ShortDeleteRequest {
            signed_event: signed(&owner, &channel, "确认"),
            ..request.clone()
        },
        ShortDeleteRequest {
            signed_event: signed(&owner, &channel, "请确认删除"),
            ..request.clone()
        },
    ] {
        assert!(store
            .validate_short_delete(invalid, "life-test")
            .await
            .is_err());
    }
    let mut tampered = request.clone();
    tampered.signed_event.content = "确认删除".into();
    tampered.signed_event.created_at = Timestamp::from(Timestamp::now().as_secs() - 30);
    assert!(store
        .validate_short_delete(tampered, "life-test")
        .await
        .is_err());
    sqlx::query("UPDATE life_pending_deletes SET expires_at=now()-interval '1 second'")
        .execute(&database.pool)
        .await
        .expect("expire");
    assert!(store
        .validate_short_delete(request.clone(), "life-test")
        .await
        .is_err());
    sqlx::query("UPDATE life_pending_deletes SET expires_at=now()+interval '5 minutes'")
        .execute(&database.pool)
        .await
        .expect("restore");
    let next_delegation = Uuid::new_v4();
    sqlx::query("INSERT INTO life_agent_delegations
        (id,token_hash,workbench_user_id,workbench_session_id,agent_id,agent_turn_id,source_event_id,source_pubkey,
         source_channel_id,audience,capabilities,data_scope,obligations,status,expires_at,max_calls,remaining_calls,trace_id)
        SELECT $1,$2,workbench_user_id,workbench_session_id,agent_id,'next-preview',$3,source_pubkey,
         source_channel_id,audience,capabilities,data_scope,obligations,status,expires_at,max_calls,remaining_calls,trace_id
         FROM life_agent_delegations WHERE id=$4")
        .bind(next_delegation).bind(vec![5u8;32]).bind("3".repeat(64)).bind(delegation)
        .execute(&database.pool).await.expect("next delegation");
    sqlx::query("INSERT INTO life_delegation_calls
        (id,delegation_id,call_id,capability,normalized_input_hash,idempotency_key,expected_version,status,trace_id)
        SELECT $1,$2,$3,capability,normalized_input_hash,idempotency_key,expected_version,status,trace_id
        FROM life_delegation_calls WHERE delegation_id=$4")
        .bind(Uuid::new_v4()).bind(next_delegation).bind(Uuid::new_v4()).bind(delegation)
        .execute(&database.pool).await.expect("next call");
    let next_preview = EventBuilder::new(Kind::Custom(9), "确认删除行动 replacement 吗？")
        .tags([
            Tag::parse(vec!["h", &channel]).expect("h"),
            Tag::parse(vec!["e", &source, "", "root"]).expect("root"),
            Tag::parse(vec!["e", &"3".repeat(64), "", "reply"]).expect("reply"),
        ])
        .custom_created_at(Timestamp::from(Timestamp::now().as_secs() - 1))
        .sign_with_keys(&agent)
        .expect("next preview");
    let command_id = Uuid::new_v4();
    store
        .record_pending_delete(PendingDeleteRequest {
            delegation_id: next_delegation,
            community_id: "community".into(),
            preview_event: next_preview,
            command_id,
            expected_version: 7,
            preview_hash: "a".repeat(64),
            expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
        })
        .await
        .expect("replace prompt");
    let active: i64 =
        sqlx::query_scalar("SELECT count(*) FROM life_pending_deletes WHERE expires_at>now()")
            .fetch_one(&database.pool)
            .await
            .expect("active previews");
    assert_eq!(active, 1);
    let signed_before_replacement = EventBuilder::new(Kind::Custom(9), "确认删除")
        .tag(Tag::parse(vec!["h", &channel]).expect("h"))
        .custom_created_at(Timestamp::from(Timestamp::now().as_secs() - 2))
        .sign_with_keys(&owner)
        .expect("old consent");
    assert!(store
        .validate_short_delete(
            ShortDeleteRequest {
                signed_event: signed_before_replacement,
                ..request.clone()
            },
            "life-test"
        )
        .await
        .is_err());
    let (first, second) = tokio::join!(
        store.validate_short_delete(request.clone(), "life-test"),
        store.validate_short_delete(request.clone(), "life-test")
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let grant = first.or(second).expect("one grant");
    assert_eq!(grant.command_id, command_id);
    assert_eq!(grant.preview_hash, "a".repeat(64));
    store
        .consume_write_confirmation(
            command_id,
            user,
            session,
            &request.signed_event.id.to_hex(),
            7,
        )
        .await
        .expect("consume");
    assert!(store
        .consume_write_confirmation(
            command_id,
            user,
            session,
            &request.signed_event.id.to_hex(),
            7
        )
        .await
        .is_err());
    assert!(store
        .validate_short_delete(request, "life-test")
        .await
        .is_err());
    database.cleanup().await;
}
