use chrono::{Duration, Utc};
use life_auth_gateway::{
    auth::OidcIdentity,
    identity::{ResolvedLifeIdentity, ResolvedMembership},
    model::{IdentityBindingId, LifeWorkbenchUserId},
    Store,
};
use nostr::{Event, EventBuilder, Keys, Kind, Timestamp};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    AssertSqlSafe, PgPool,
};
use std::str::FromStr;
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
            .expect("connect test database");
        let schema = format!("life_auth_test_{}", Uuid::new_v4().simple());
        sqlx::raw_sql(AssertSqlSafe(format!("CREATE SCHEMA \"{schema}\"")))
            .execute(&admin)
            .await
            .expect("create isolated schema");
        let options = PgConnectOptions::from_str(&url)
            .expect("parse test database URL")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(6)
            .connect_with(options)
            .await
            .expect("connect isolated schema");
        Store::migrate(&pool).await.expect("run Life migrations");
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
        .expect("drop isolated schema");
        self.admin.close().await;
    }
}

fn oidc(subject: &str) -> OidcIdentity {
    OidcIdentity {
        issuer: "https://identity.example/application/o/life/".into(),
        subject: subject.into(),
        expires_at: Utc::now() + Duration::hours(1),
    }
}

fn resolved(user: &str, workspace: &str) -> ResolvedLifeIdentity {
    ResolvedLifeIdentity {
        life_os_user_id: user.into(),
        active: true,
        memberships: vec![ResolvedMembership {
            workspace_id: workspace.into(),
            role: "OWNER".into(),
            membership_version: 1,
        }],
    }
}

async fn session(
    store: &Store,
    subject: &str,
    user: &str,
) -> life_auth_gateway::identity::IssuedSession {
    store
        .create_workbench_session(
            &oidc(subject),
            &resolved(user, "workspace-1"),
            "life-test",
            Uuid::new_v4(),
        )
        .await
        .expect("create Workbench session")
}

fn tamper_event_id(event: &Event) -> Event {
    let mut value = serde_json::to_value(event).expect("serialize event");
    value["id"] = serde_json::Value::String("0".repeat(64));
    serde_json::from_value(value).expect("deserialize tampered event")
}

#[tokio::test]
async fn signed_challenge_is_session_scoped_single_use_and_pubkey_unique() {
    let Some(database) = TestDatabase::create().await else {
        eprintln!("LIFE_AUTH_TEST_DATABASE_URL absent; identity integration test skipped");
        return;
    };
    let store = Store::new(database.pool.clone());
    let first = session(&store, "subject-a", "life-user-a").await;
    let second = session(&store, "subject-b", "life-user-b").await;
    let first_principal = store
        .authenticate_workbench_session(&first.session_token, "life-test")
        .await
        .expect("authenticate first session");
    let second_principal = store
        .authenticate_workbench_session(&second.session_token, "life-test")
        .await
        .expect("authenticate second session");
    let keys = Keys::generate();

    let wrong_signer_challenge = store
        .create_identity_binding_challenge(
            &first_principal,
            &keys.public_key().to_hex(),
            Duration::seconds(90),
            Uuid::new_v4(),
        )
        .await
        .expect("wrong-signer challenge");
    let wrong_signer = EventBuilder::new(
        Kind::Custom(24243),
        &wrong_signer_challenge.canonical_payload,
    )
    .sign_with_keys(&Keys::generate())
    .expect("sign with wrong key");
    assert!(store
        .verify_identity_binding(
            &first_principal,
            wrong_signer_challenge.challenge_id,
            wrong_signer,
            Uuid::new_v4(),
        )
        .await
        .is_err());

    let expired_challenge = store
        .create_identity_binding_challenge(
            &first_principal,
            &keys.public_key().to_hex(),
            Duration::seconds(90),
            Uuid::new_v4(),
        )
        .await
        .expect("expired challenge");
    sqlx::query(
        "UPDATE life_identity_binding_challenges
         SET created_at=now()-interval '2 minutes',expires_at=now()-interval '1 second'
         WHERE id=$1",
    )
    .bind(expired_challenge.challenge_id.as_uuid())
    .execute(&database.pool)
    .await
    .expect("expire challenge");
    let expired_event =
        EventBuilder::new(Kind::Custom(24243), &expired_challenge.canonical_payload)
            .sign_with_keys(&keys)
            .expect("sign expired challenge");
    assert!(store
        .verify_identity_binding(
            &first_principal,
            expired_challenge.challenge_id,
            expired_event,
            Uuid::new_v4(),
        )
        .await
        .is_err());

    let wrong_kind_challenge = store
        .create_identity_binding_challenge(
            &first_principal,
            &keys.public_key().to_hex(),
            Duration::seconds(90),
            Uuid::new_v4(),
        )
        .await
        .expect("wrong-kind challenge");
    let wrong_kind = EventBuilder::new(Kind::TextNote, &wrong_kind_challenge.canonical_payload)
        .sign_with_keys(&keys)
        .expect("sign wrong kind");
    assert!(store
        .verify_identity_binding(
            &first_principal,
            wrong_kind_challenge.challenge_id,
            wrong_kind,
            Uuid::new_v4(),
        )
        .await
        .is_err());

    let tampered_challenge = store
        .create_identity_binding_challenge(
            &first_principal,
            &keys.public_key().to_hex(),
            Duration::seconds(90),
            Uuid::new_v4(),
        )
        .await
        .expect("tampered-ID challenge");
    let signed = EventBuilder::new(Kind::Custom(24243), &tampered_challenge.canonical_payload)
        .sign_with_keys(&keys)
        .expect("sign event");
    assert!(store
        .verify_identity_binding(
            &first_principal,
            tampered_challenge.challenge_id,
            tamper_event_id(&signed),
            Uuid::new_v4(),
        )
        .await
        .is_err());

    let stale_challenge = store
        .create_identity_binding_challenge(
            &first_principal,
            &keys.public_key().to_hex(),
            Duration::seconds(90),
            Uuid::new_v4(),
        )
        .await
        .expect("stale-event challenge");
    let stale_event = EventBuilder::new(Kind::Custom(24243), &stale_challenge.canonical_payload)
        .custom_created_at(Timestamp::from(Utc::now().timestamp() as u64 - 300))
        .sign_with_keys(&keys)
        .expect("sign stale event");
    assert!(store
        .verify_identity_binding(
            &first_principal,
            stale_challenge.challenge_id,
            stale_event,
            Uuid::new_v4(),
        )
        .await
        .is_err());

    let challenge = store
        .create_identity_binding_challenge(
            &first_principal,
            &keys.public_key().to_hex(),
            Duration::seconds(90),
            Uuid::new_v4(),
        )
        .await
        .expect("binding challenge");
    assert!(challenge
        .canonical_payload
        .contains(&first.session_id.as_uuid().to_string()));
    assert!(challenge
        .canonical_payload
        .contains("deployment_id=life-test"));
    let event = EventBuilder::new(Kind::Custom(24243), &challenge.canonical_payload)
        .sign_with_keys(&keys)
        .expect("sign binding event");
    assert!(store
        .verify_identity_binding(
            &second_principal,
            challenge.challenge_id,
            event.clone(),
            Uuid::new_v4(),
        )
        .await
        .is_err());
    let binding = store
        .verify_identity_binding(
            &first_principal,
            challenge.challenge_id,
            event.clone(),
            Uuid::new_v4(),
        )
        .await
        .expect("bind pubkey");
    assert!(store
        .verify_identity_binding(
            &first_principal,
            challenge.challenge_id,
            event,
            Uuid::new_v4(),
        )
        .await
        .is_err());

    let conflict = store
        .create_identity_binding_challenge(
            &second_principal,
            &keys.public_key().to_hex(),
            Duration::seconds(90),
            Uuid::new_v4(),
        )
        .await
        .expect("conflicting challenge");
    let conflict_event = EventBuilder::new(Kind::Custom(24243), &conflict.canonical_payload)
        .sign_with_keys(&keys)
        .expect("sign conflicting event");
    assert!(store
        .verify_identity_binding(
            &second_principal,
            conflict.challenge_id,
            conflict_event,
            Uuid::new_v4(),
        )
        .await
        .is_err());

    seed_binding_dependants(&database.pool, binding.binding_id, first.user_id).await;
    store
        .revoke_identity_binding(&first_principal, binding.binding_id, Uuid::new_v4())
        .await
        .expect("revoke binding");
    assert_eq!(
        active_dependants(&database.pool, binding.binding_id).await,
        0
    );
    let audit_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM life_security_audit
         WHERE event_type='IDENTITY_BINDING_REVOKED' AND outcome='success'",
    )
    .fetch_one(&database.pool)
    .await
    .expect("count revocation audit");
    assert_eq!(audit_count, 1);

    database.cleanup().await;
}

async fn seed_binding_dependants(
    pool: &PgPool,
    binding_id: IdentityBindingId,
    user_id: LifeWorkbenchUserId,
) {
    let session_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM life_workbench_sessions WHERE workbench_user_id=$1 LIMIT 1",
    )
    .bind(user_id.as_uuid())
    .fetch_one(pool)
    .await
    .expect("session id");
    sqlx::query(
        "INSERT INTO life_agent_delegations
         (id,token_hash,workbench_user_id,workbench_session_id,identity_binding_id,
          agent_id,agent_turn_id,source_event_id,source_pubkey,audience,capabilities,
          data_scope,obligations,status,expires_at,max_calls,remaining_calls,trace_id)
         VALUES($1,$2,$3,$4,$5,'agent','turn',$6,$7,'life-workbench-mcp','[]','{}','[]',
                'active',now()+interval '5 minutes',1,1,$8)",
    )
    .bind(Uuid::new_v4())
    .bind(vec![31_u8; 32])
    .bind(user_id.as_uuid())
    .bind(session_id)
    .bind(binding_id.as_uuid())
    .bind("d".repeat(64))
    .bind("e".repeat(64))
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .expect("seed delegation");
    sqlx::query(
        "INSERT INTO life_embed_sessions
         (id,session_token_hash,workbench_user_id,workbench_session_id,identity_binding_id,
          deployment_id,status,expires_at,trace_id)
         VALUES($1,$2,$3,$4,$5,'life-test','active',now()+interval '5 minutes',$6)",
    )
    .bind(Uuid::new_v4())
    .bind(vec![32_u8; 32])
    .bind(user_id.as_uuid())
    .bind(session_id)
    .bind(binding_id.as_uuid())
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .expect("seed embed session");
}

async fn active_dependants(pool: &PgPool, binding_id: IdentityBindingId) -> i64 {
    sqlx::query_scalar(
        "SELECT
           (SELECT count(*) FROM life_agent_delegations WHERE identity_binding_id=$1 AND status='active') +
           (SELECT count(*) FROM life_embed_sessions WHERE identity_binding_id=$1 AND status='active')",
    )
    .bind(binding_id.as_uuid())
    .fetch_one(pool)
    .await
    .expect("count active dependants")
}
