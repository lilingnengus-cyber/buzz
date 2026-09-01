use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use life_auth_gateway::{
    agent::{
        ConsumeDelegationRequest, ConversationAudience, DelegationPolicy, IssueDelegationRequest,
        RequestedDataScope, ResourceContext,
    },
    call_grant::CallGrantSigner,
    identity::{ResolvedLifeIdentity, ResolvedMembership},
    membership::{MembershipEvent, MembershipSnapshot},
    security::SigningKeyMaterial,
    Store,
};
use life_iam::Capability;
use nostr::{EventBuilder, Keys, Kind, Tag, TagKind, Timestamp};
use sha2::{Digest, Sha256};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    AssertSqlSafe, PgPool, Row,
};
use std::{str::FromStr, time::Duration};
use uuid::Uuid;

struct TestDatabase {
    admin: PgPool,
    pool: PgPool,
    schema: String,
}

impl TestDatabase {
    async fn create() -> Option<Self> {
        let url = std::env::var("LIFE_AUTH_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .ok()?;
        let schema = format!("life_auth_test_{}", Uuid::new_v4().simple());
        sqlx::raw_sql(AssertSqlSafe(format!("CREATE SCHEMA \"{schema}\"")))
            .execute(&admin)
            .await
            .expect("create schema");
        let options = PgConnectOptions::from_str(&url)
            .expect("database URL")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(12)
            .connect_with(options)
            .await
            .expect("schema pool");
        Store::migrate(&pool).await.expect("migrations");
        Some(Self {
            admin,
            pool,
            schema,
        })
    }

    async fn cleanup(self) {
        self.pool.close().await;
        sqlx::raw_sql(AssertSqlSafe(format!(
            "DROP SCHEMA \"{}\" CASCADE",
            self.schema
        )))
        .execute(&self.admin)
        .await
        .expect("drop schema");
        self.admin.close().await;
    }
}

async fn seed_authority(pool: &PgPool, keys: &Keys) -> Store {
    let user_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO life_workbench_users
         (id,oidc_issuer,oidc_subject,life_os_user_id,status)
         VALUES($1,'https://identity.example','delegator','life-user','active')",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .expect("user");
    sqlx::query(
        "INSERT INTO life_workbench_sessions
         (id,workbench_user_id,deployment_id,token_hash,status,expires_at)
         VALUES($1,$2,'life-test',$3,'active',now()+interval '1 hour')",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(vec![61_u8; 32])
    .execute(pool)
    .await
    .expect("session");
    sqlx::query(
        "INSERT INTO life_identity_bindings
         (id,workbench_user_id,buzz_pubkey,source_event_id,status)
         VALUES($1,$2,$3,$4,'active')",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(keys.public_key().to_hex())
    .bind("a".repeat(64))
    .execute(pool)
    .await
    .expect("binding");
    let store = Store::new(pool.clone());
    store
        .apply_membership_event(&MembershipEvent {
            life_os_user_id: "life-user".into(),
            user_active: true,
            membership_version: 1,
            memberships: vec![MembershipSnapshot {
                workspace_id: "workspace-1".into(),
                role: "OWNER".into(),
            }],
            trace_id: Uuid::new_v4(),
        })
        .await
        .expect("membership");
    store
}

fn policy() -> DelegationPolicy {
    DelegationPolicy::new("life-workbench-mcp", "life-test", Duration::from_secs(300))
        .expect("policy")
}

fn current_identity() -> ResolvedLifeIdentity {
    ResolvedLifeIdentity {
        life_os_user_id: "life-user".into(),
        active: true,
        memberships: vec![ResolvedMembership {
            workspace_id: "workspace-1".into(),
            role: "OWNER".into(),
            membership_version: 9,
        }],
    }
}

fn source(keys: &Keys, channel: Uuid) -> nostr::Event {
    EventBuilder::new(Kind::Custom(40002), "update my action")
        .tags([Tag::custom(
            TagKind::Custom("h".into()),
            [channel.to_string()],
        )])
        .sign_with_keys(keys)
        .expect("signed source")
}

fn issue_request(event: nostr::Event, channel: Uuid, turn: &str) -> IssueDelegationRequest {
    IssueDelegationRequest {
        source_event: event,
        source_channel_id: Some(channel.to_string()),
        conversation: ConversationAudience::Channel {
            participant_pubkeys: vec![],
            direct_message: false,
        },
        agent_id: "life-agent".into(),
        agent_turn_id: turn.into(),
        requested_capabilities: vec!["action:status_update".into()],
        requested_data_scope: RequestedDataScope {
            workspace: vec!["workspace-1".into(), "workspace-2".into()],
            resource: vec!["action-1".into()],
            ..RequestedDataScope::default()
        },
        resource_context: Some(ResourceContext {
            resource_type: "action".into(),
            id: "action-1".into(),
            expected_version: Some(7),
            preview_hash: None,
        }),
        write_command_id: None,
        trace_id: Uuid::new_v4(),
    }
}

#[tokio::test]
async fn signed_source_issues_hash_only_scoped_consumable_grant() {
    let Some(database) = TestDatabase::create().await else {
        eprintln!("LIFE_AUTH_TEST_DATABASE_URL absent; delegation test skipped");
        return;
    };
    let keys = Keys::generate();
    let store = seed_authority(&database.pool, &keys).await;
    assert!(
        DelegationPolicy::new("wrong-audience", "life-test", Duration::from_secs(300)).is_err()
    );
    assert!(
        DelegationPolicy::new("life-workbench-mcp", "life-test", Duration::from_secs(29)).is_err()
    );
    assert!(
        DelegationPolicy::new("life-workbench-mcp", "life-test", Duration::from_secs(901)).is_err()
    );
    let channel = Uuid::new_v4();
    let source = source(&keys, channel);
    let issued = store
        .issue_agent_delegation(
            issue_request(source.clone(), channel, "turn-1"),
            &policy(),
            &current_identity(),
        )
        .await
        .expect("issue delegation");
    assert_eq!(
        URL_SAFE_NO_PAD.decode(&issued.token).expect("token").len(),
        32
    );
    assert_eq!(issued.audience, "life-workbench-mcp");
    let ttl = issued.expires_at - chrono::Utc::now();
    assert!(ttl.num_seconds() >= 298 && ttl.num_seconds() <= 300);
    assert_eq!(issued.max_calls, 1, "writes are single-call delegations");
    assert_eq!(
        issued.effective_capabilities,
        vec![Capability::parse("action:status_update").unwrap()]
    );
    assert_eq!(issued.effective_data_scope.workspace, vec!["workspace-1"]);
    let row = sqlx::query(
        "SELECT token_hash,source_event_id,identity_binding_id,iam_decision_id,
                source_channel_id,catalog_version,status,remaining_calls
         FROM life_agent_delegations WHERE id=$1",
    )
    .bind(issued.delegation_id.as_uuid())
    .fetch_one(&database.pool)
    .await
    .expect("delegation row");
    assert_eq!(
        row.get::<Vec<u8>, _>("token_hash"),
        Sha256::digest(issued.token.as_bytes()).to_vec()
    );
    assert_ne!(row.get::<Vec<u8>, _>("token_hash"), issued.token.as_bytes());
    assert_eq!(row.get::<String, _>("source_event_id"), source.id.to_hex());
    assert_eq!(
        row.get::<Uuid, _>("iam_decision_id"),
        issued.iam_decision_id
    );
    assert_eq!(row.get::<i32, _>("catalog_version"), 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT authority_version FROM life_workbench_users WHERE life_os_user_id='life-user'",
        )
        .fetch_one(&database.pool)
        .await
        .expect("authority version"),
        9,
        "an unchanged authoritative snapshot still advances the version watermark"
    );

    let duplicate = store
        .issue_agent_delegation(
            issue_request(source, channel, "turn-2"),
            &policy(),
            &current_identity(),
        )
        .await;
    assert!(
        duplicate.is_err(),
        "one source event cannot authorize twice"
    );

    let signer = CallGrantSigner::new(
        "life-auth-test",
        "lifeos-workbench-api",
        Duration::from_secs(30),
        SigningKeyMaterial::parse(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let wrong_resource = store
        .consume_agent_delegation(
            &issued.token,
            ConsumeDelegationRequest {
                agent_id: "life-agent".into(),
                agent_turn_id: "turn-1".into(),
                tool: "update_action_status".into(),
                capability: "action:status_update".into(),
                resource: Some(ResourceContext {
                    resource_type: "action".into(),
                    id: "action-other".into(),
                    expected_version: Some(7),
                    preview_hash: None,
                }),
                normalized_input_hash: format!("sha256:{}", "b".repeat(64)),
                idempotency_key: Uuid::new_v4().to_string(),
                trace_id: issued.trace_id,
            },
            &signer,
        )
        .await;
    assert!(wrong_resource.is_err());
    assert_eq!(
        sqlx::query_scalar::<_, i32>(
            "SELECT remaining_calls FROM life_agent_delegations WHERE id=$1",
        )
        .bind(issued.delegation_id.as_uuid())
        .fetch_one(&database.pool)
        .await
        .unwrap(),
        1
    );
    let consumed = store
        .consume_agent_delegation(
            &issued.token,
            ConsumeDelegationRequest {
                agent_id: "life-agent".into(),
                agent_turn_id: "turn-1".into(),
                tool: "update_action_status".into(),
                capability: "action:status_update".into(),
                resource: Some(ResourceContext {
                    resource_type: "action".into(),
                    id: "action-1".into(),
                    expected_version: Some(7),
                    preview_hash: None,
                }),
                normalized_input_hash: format!("sha256:{}", "b".repeat(64)),
                idempotency_key: Uuid::new_v4().to_string(),
                trace_id: issued.trace_id,
            },
            &signer,
        )
        .await
        .expect("consume delegation");
    assert_eq!(
        consumed.claims.delegation_id,
        issued.delegation_id.as_uuid()
    );
    assert_eq!(consumed.claims.life_os_user_id, "life-user");
    assert_eq!(consumed.claims.capability, "action:status_update");
    assert_eq!(consumed.claims.expected_version, Some(7));
    assert_eq!(consumed.token.split('.').count(), 3);
    let parts = consumed.token.split('.').collect::<Vec<_>>();
    let payload: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).expect("grant payload"))
            .expect("grant claims JSON");
    assert_eq!(payload["lifeOsUserId"], "life-user");
    let signature = Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).expect("signature"))
        .expect("Ed25519 signature");
    VerifyingKey::from_bytes(&signer.verifying_key_bytes())
        .expect("verification key")
        .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
        .expect("valid LifeCallGrant signature");
    let status =
        sqlx::query("SELECT status,remaining_calls FROM life_agent_delegations WHERE id=$1")
            .bind(issued.delegation_id.as_uuid())
            .fetch_one(&database.pool)
            .await
            .expect("consumed state");
    assert_eq!(status.get::<String, _>("status"), "exhausted");
    assert_eq!(status.get::<i32, _>("remaining_calls"), 0);
    assert!(store
        .consume_agent_delegation(
            &issued.token,
            ConsumeDelegationRequest {
                agent_id: "life-agent".into(),
                agent_turn_id: "turn-1".into(),
                tool: "update_action_status".into(),
                capability: "action:status_update".into(),
                resource: Some(ResourceContext {
                    resource_type: "action".into(),
                    id: "action-1".into(),
                    expected_version: Some(7),
                    preview_hash: None,
                }),
                normalized_input_hash: format!("sha256:{}", "b".repeat(64)),
                idempotency_key: Uuid::new_v4().to_string(),
                trace_id: issued.trace_id,
            },
            &signer,
        )
        .await
        .is_err());

    database.cleanup().await;
}

#[tokio::test]
async fn buzz_dm_exact_confirmation_satisfies_step_up_and_binds_one_execute_call() {
    let Some(database) = TestDatabase::create().await else {
        return;
    };
    let keys = Keys::generate();
    let store = seed_authority(&database.pool, &keys).await;
    let channel = Uuid::new_v4();
    let command_id = Uuid::new_v4();
    let preview_hash = "c".repeat(64);
    let source = EventBuilder::new(
        Kind::Custom(40002),
        format!("/confirm life-write {command_id} v7 {preview_hash}"),
    )
    .tags([Tag::custom(
        TagKind::Custom("h".into()),
        [channel.to_string()],
    )])
    .sign_with_keys(&keys)
    .expect("signed confirmation");
    let identity = sqlx::query(
        "SELECT u.id AS user_id,s.id AS session_id
         FROM life_workbench_users u
         JOIN life_workbench_sessions s ON s.workbench_user_id=u.id
         WHERE u.life_os_user_id='life-user' AND s.status='active'",
    )
    .fetch_one(&database.pool)
    .await
    .expect("identity");
    sqlx::query(
        "INSERT INTO life_write_command_confirmations
         (id,command_id,workbench_user_id,workbench_session_id,source_event_id,
          expected_version,preview_hash,status,expires_at,trace_id)
         VALUES($1,$2,$3,$4,$5,7,$6,'active',now()+interval '10 minutes',$7)",
    )
    .bind(Uuid::new_v4())
    .bind(command_id)
    .bind(identity.get::<Uuid, _>("user_id"))
    .bind(identity.get::<Uuid, _>("session_id"))
    .bind(source.id.to_hex())
    .bind(hex::decode(&preview_hash).expect("preview hash"))
    .bind(Uuid::new_v4())
    .execute(&database.pool)
    .await
    .expect("confirmation grant");

    let issued = store
        .issue_agent_delegation(
            IssueDelegationRequest {
                source_event: source,
                source_channel_id: Some(channel.to_string()),
                conversation: ConversationAudience::Channel {
                    participant_pubkeys: vec![],
                    direct_message: true,
                },
                agent_id: "life-agent".into(),
                agent_turn_id: "confirmed-turn".into(),
                requested_capabilities: vec!["write_command:execute".into()],
                requested_data_scope: RequestedDataScope {
                    workspace: vec!["workspace-1".into()],
                    resource: vec![command_id.to_string()],
                    ..RequestedDataScope::default()
                },
                resource_context: Some(ResourceContext {
                    resource_type: "write_command".into(),
                    id: command_id.to_string(),
                    expected_version: Some(7),
                    preview_hash: Some(preview_hash.clone()),
                }),
                write_command_id: Some(command_id),
                trace_id: Uuid::new_v4(),
            },
            &policy(),
            &current_identity(),
        )
        .await
        .expect("high-risk delegation");
    assert_eq!(issued.max_calls, 1);
    assert_eq!(
        issued.effective_capabilities,
        vec![Capability::parse("write_command:execute").expect("capability")]
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT status FROM life_write_command_confirmations WHERE command_id=$1",
        )
        .bind(command_id)
        .fetch_one(&database.pool)
        .await
        .expect("confirmation status"),
        "consumed"
    );

    let signer = CallGrantSigner::new(
        "life-auth-test",
        "lifeos-workbench-api",
        Duration::from_secs(30),
        SigningKeyMaterial::parse(&"33".repeat(32)).expect("signing key"),
    )
    .expect("signer");
    let grant = store
        .consume_agent_delegation(
            &issued.token,
            ConsumeDelegationRequest {
                agent_id: "life-agent".into(),
                agent_turn_id: "confirmed-turn".into(),
                tool: "execute_confirmed_life_write".into(),
                capability: "write_command:execute".into(),
                resource: None,
                normalized_input_hash: format!("sha256:{}", "0".repeat(64)),
                idempotency_key: Uuid::new_v4().to_string(),
                trace_id: issued.trace_id,
            },
            &signer,
        )
        .await
        .expect("consume confirmed execution");
    assert_eq!(grant.claims.resource_type, "write_command");
    assert_eq!(grant.claims.resource_id, command_id.to_string());
    assert_eq!(grant.claims.expected_version, Some(7));
    assert_eq!(grant.claims.preview_hash, Some(preview_hash));

    database.cleanup().await;
}

#[tokio::test]
async fn invalid_source_kind_author_time_channel_and_dm_participants_fail_closed() {
    let Some(database) = TestDatabase::create().await else {
        return;
    };
    let keys = Keys::generate();
    let store = seed_authority(&database.pool, &keys).await;
    let channel = Uuid::new_v4();
    let other_keys = Keys::generate();
    let invalid_author = source(&other_keys, channel);
    assert!(store
        .issue_agent_delegation(
            issue_request(invalid_author, channel, "bad-author"),
            &policy(),
            &current_identity(),
        )
        .await
        .is_err());
    let mut tampered = source(&keys, channel);
    tampered.content.push_str(" tampered");
    assert!(store
        .issue_agent_delegation(
            issue_request(tampered, channel, "bad-signature"),
            &policy(),
            &current_identity(),
        )
        .await
        .is_err());
    let invalid_kind = EventBuilder::new(Kind::Custom(24243), "not a message")
        .sign_with_keys(&keys)
        .unwrap();
    assert!(store
        .issue_agent_delegation(
            issue_request(invalid_kind, channel, "bad-kind"),
            &policy(),
            &current_identity(),
        )
        .await
        .is_err());
    let old = EventBuilder::new(Kind::Custom(40002), "old")
        .custom_created_at(Timestamp::from_secs(1))
        .tags([Tag::custom(
            TagKind::Custom("h".into()),
            [channel.to_string()],
        )])
        .sign_with_keys(&keys)
        .unwrap();
    assert!(store
        .issue_agent_delegation(
            issue_request(old, channel, "old"),
            &policy(),
            &current_identity(),
        )
        .await
        .is_err());
    let wrong_channel = source(&keys, Uuid::new_v4());
    assert!(store
        .issue_agent_delegation(
            issue_request(wrong_channel, channel, "bad-channel"),
            &policy(),
            &current_identity(),
        )
        .await
        .is_err());

    let recipient_keys = Keys::generate();
    let recipient = recipient_keys.public_key().to_hex();
    let dm = EventBuilder::new(Kind::TextNote, "private request")
        .tags([Tag::public_key(Keys::generate().public_key())])
        .sign_with_keys(&keys)
        .unwrap();
    let mut dm_request = issue_request(dm, channel, "bad-dm");
    dm_request.source_channel_id = None;
    dm_request.conversation = ConversationAudience::DirectMessage {
        participant_pubkeys: vec![keys.public_key().to_hex(), recipient.clone()],
    };
    assert!(store
        .issue_agent_delegation(dm_request, &policy(), &current_identity())
        .await
        .is_err());

    let valid_dm = EventBuilder::new(Kind::TextNote, "private request")
        .tags([Tag::public_key(recipient_keys.public_key())])
        .sign_with_keys(&keys)
        .unwrap();
    let mut valid_dm_request = issue_request(valid_dm, channel, "valid-dm");
    valid_dm_request.source_channel_id = None;
    valid_dm_request.conversation = ConversationAudience::DirectMessage {
        participant_pubkeys: vec![keys.public_key().to_hex(), recipient],
    };
    let valid_dm_issued = store
        .issue_agent_delegation(valid_dm_request, &policy(), &current_identity())
        .await
        .expect("valid DM delegation");
    let mut disabled_identity = current_identity();
    disabled_identity.active = false;
    disabled_identity.memberships.clear();
    let disabled_source = source(&keys, Uuid::new_v4());
    let disabled_channel = event_tag_value(&disabled_source, "h");
    assert!(store
        .issue_agent_delegation(
            issue_request(disabled_source, disabled_channel, "disabled-user"),
            &policy(),
            &disabled_identity,
        )
        .await
        .is_err());
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM life_agent_delegations WHERE id=$1")
            .bind(valid_dm_issued.delegation_id.as_uuid())
            .fetch_one(&database.pool)
            .await
            .unwrap(),
        "revoked"
    );
    database.cleanup().await;
}

fn event_tag_value(event: &nostr::Event, name: &str) -> Uuid {
    event
        .tags
        .iter()
        .find_map(|tag| {
            let values = tag.as_slice();
            (values.first().is_some_and(|value| value == name))
                .then(|| values.get(1))
                .flatten()
        })
        .and_then(|value| Uuid::parse_str(value).ok())
        .expect("event UUID tag")
}
