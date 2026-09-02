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
use nostr::{EventBuilder, Keys, Kind, Tag, TagKind};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    AssertSqlSafe, PgPool, Row,
};
use std::{str::FromStr, sync::Arc, time::Duration};
use tokio::sync::Barrier;
use uuid::Uuid;

struct Database {
    admin: PgPool,
    pool: PgPool,
    schema: String,
}

impl Database {
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
            .ok()?;
        let options = PgConnectOptions::from_str(&url)
            .ok()?
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(24)
            .connect_with(options)
            .await
            .ok()?;
        Store::migrate(&pool).await.ok()?;
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
        .unwrap();
        self.admin.close().await;
    }
}

async fn setup(database: &Database) -> (Store, Keys) {
    let keys = Keys::generate();
    let user = Uuid::new_v4();
    let session = Uuid::new_v4();
    sqlx::query("INSERT INTO life_workbench_users(id,oidc_issuer,oidc_subject,life_os_user_id,status) VALUES($1,'https://identity.example','race','race-user','active')")
        .bind(user).execute(&database.pool).await.unwrap();
    sqlx::query("INSERT INTO life_workbench_sessions(id,workbench_user_id,deployment_id,token_hash,status,expires_at) VALUES($1,$2,'life-test',$3,'active',now()+interval '1 hour')")
        .bind(session).bind(user).bind(vec![71_u8;32]).execute(&database.pool).await.unwrap();
    sqlx::query("INSERT INTO life_identity_bindings(id,workbench_user_id,buzz_pubkey,source_event_id,status) VALUES($1,$2,$3,$4,'active')")
        .bind(Uuid::new_v4()).bind(user).bind(keys.public_key().to_hex()).bind("c".repeat(64)).execute(&database.pool).await.unwrap();
    let store = Store::new(database.pool.clone());
    store
        .apply_membership_event(&MembershipEvent {
            life_os_user_id: "race-user".into(),
            user_active: true,
            membership_version: 1,
            memberships: vec![MembershipSnapshot {
                workspace_id: "workspace-1".into(),
                role: "OWNER".into(),
            }],
            trace_id: Uuid::new_v4(),
        })
        .await
        .unwrap();
    (store, keys)
}

fn request(keys: &Keys, turn: &str) -> IssueDelegationRequest {
    let channel = Uuid::new_v4();
    let event = EventBuilder::new(Kind::Custom(40002), "read actions")
        .tags([Tag::custom(
            TagKind::Custom("h".into()),
            [channel.to_string()],
        )])
        .sign_with_keys(keys)
        .unwrap();
    IssueDelegationRequest {
        source_event: event,
        source_channel_id: Some(channel.to_string()),
        conversation: ConversationAudience::Channel {
            participant_pubkeys: vec![],
            direct_message: false,
        },
        agent_id: "race-agent".into(),
        agent_turn_id: turn.into(),
        requested_capabilities: vec!["action:read".into()],
        requested_data_scope: RequestedDataScope {
            workspace: vec!["workspace-1".into()],
            resource: vec!["action-1".into()],
            ..Default::default()
        },
        resource_context: Some(ResourceContext {
            resource_type: "action".into(),
            id: "action-1".into(),
            expected_version: None,
            preview_hash: None,
        }),
        write_command_id: None,
        trace_id: Uuid::new_v4(),
    }
}

fn current_identity() -> ResolvedLifeIdentity {
    ResolvedLifeIdentity {
        life_os_user_id: "race-user".into(),
        active: true,
        memberships: vec![ResolvedMembership {
            workspace_id: "workspace-1".into(),
            role: "OWNER".into(),
            membership_version: 1,
        }],
    }
}

fn consume(turn: &str, trace_id: Uuid) -> ConsumeDelegationRequest {
    ConsumeDelegationRequest {
        agent_id: "race-agent".into(),
        agent_turn_id: turn.into(),
        tool: "get_action_detail".into(),
        capability: "action:read".into(),
        resource: Some(ResourceContext {
            resource_type: "action".into(),
            id: "action-1".into(),
            expected_version: None,
            preview_hash: None,
        }),
        normalized_input_hash: format!("sha256:{}", "d".repeat(64)),
        idempotency_key: Uuid::new_v4().to_string(),
        trace_id,
    }
}

#[tokio::test]
async fn call_budget_and_revoke_are_serialized_and_expiry_is_terminal() {
    let Some(database) = Database::create().await else {
        return;
    };
    let (store, keys) = setup(&database).await;
    let policy =
        DelegationPolicy::new("life-workbench-mcp", "life-test", Duration::from_secs(300)).unwrap();
    let signer = CallGrantSigner::new(
        "issuer",
        "lifeos-workbench-api",
        Duration::from_secs(30),
        SigningKeyMaterial::parse(&"22".repeat(32)).unwrap(),
    )
    .unwrap();
    let issued = store
        .issue_agent_delegation(request(&keys, "race-turn"), &policy, &current_identity())
        .await
        .unwrap();
    let barrier = Arc::new(Barrier::new(34));
    let mut tasks = Vec::new();
    for _ in 0..32 {
        let store = store.clone();
        let token = issued.token.clone();
        let signer = signer.clone();
        let barrier = barrier.clone();
        let trace = issued.trace_id;
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            store
                .consume_agent_delegation(&token, consume("race-turn", trace), &signer)
                .await
                .is_ok()
        }));
    }
    let revoke_store = store.clone();
    let revoke_barrier = barrier.clone();
    let delegation_id = issued.delegation_id;
    let revoke = tokio::spawn(async move {
        revoke_barrier.wait().await;
        revoke_store.revoke_agent_delegation(delegation_id).await
    });
    barrier.wait().await;
    revoke.await.unwrap().unwrap();
    let mut successes = 0;
    for task in tasks {
        if task.await.unwrap() {
            successes += 1;
        }
    }
    assert!(successes <= issued.max_calls);
    assert!(store
        .consume_agent_delegation(
            &issued.token,
            consume("race-turn", issued.trace_id),
            &signer
        )
        .await
        .is_err());
    let row = sqlx::query("SELECT status,remaining_calls FROM life_agent_delegations WHERE id=$1")
        .bind(issued.delegation_id.as_uuid())
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("status"), "revoked");
    assert!(row.get::<i32, _>("remaining_calls") >= 0);

    let expired = store
        .issue_agent_delegation(request(&keys, "expired-turn"), &policy, &current_identity())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE life_agent_delegations
         SET created_at=now()-interval '1 minute',expires_at=now()-interval '1 second'
         WHERE id=$1",
    )
    .bind(expired.delegation_id.as_uuid())
    .execute(&database.pool)
    .await
    .unwrap();
    assert!(store
        .consume_agent_delegation(
            &expired.token,
            consume("expired-turn", expired.trace_id),
            &signer
        )
        .await
        .is_err());
    let expired_status: String =
        sqlx::query_scalar("SELECT status FROM life_agent_delegations WHERE id=$1")
            .bind(expired.delegation_id.as_uuid())
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert_eq!(expired_status, "expired");
    database.cleanup().await;
}
