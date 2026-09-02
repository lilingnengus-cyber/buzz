use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use life_auth_gateway::{
    embed::{EmbedPolicy, EmbedRiskFacts, IssueEmbedRequest},
    identity::SessionPrincipal,
    model::{LifeWorkbenchUserId, WorkbenchSessionId},
    Store,
};
use sha2::{Digest, Sha256};
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
        .expect("drop test schema");
        self.admin.close().await;
    }
}

async fn principal(pool: &PgPool, subject: &str) -> SessionPrincipal {
    let user_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO life_workbench_users
         (id,oidc_issuer,oidc_subject,life_os_user_id,status)
         VALUES($1,'https://identity.example',$2,$3,'active')",
    )
    .bind(user_id)
    .bind(subject)
    .bind(format!("life-{subject}"))
    .execute(pool)
    .await
    .expect("user");
    sqlx::query(
        "INSERT INTO life_workbench_sessions
         (id,workbench_user_id,deployment_id,token_hash,status,expires_at)
         VALUES($1,$2,'life-test',$3,'active',now()+interval '2 hours')",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(Sha256::digest(format!("session-{subject}")).to_vec())
    .execute(pool)
    .await
    .expect("session");
    SessionPrincipal {
        session_id: WorkbenchSessionId::new(session_id),
        user_id: LifeWorkbenchUserId::new(user_id),
        issuer: "https://identity.example".into(),
        subject: subject.into(),
        life_os_user_id: format!("life-{subject}"),
        deployment_id: "life-test".into(),
    }
}

fn policy() -> EmbedPolicy {
    EmbedPolicy::new(Duration::from_secs(30), Duration::from_secs(3600)).expect("valid policy")
}

fn risk_facts() -> EmbedRiskFacts {
    EmbedRiskFacts::from_request(Some("192.0.2.10"), Some("Pacioli-Test/1"))
}

#[tokio::test]
async fn embed_code_is_hash_only_bound_and_has_one_concurrent_consumer() {
    let Some(database) = Database::create().await else {
        eprintln!("LIFE_AUTH_TEST_DATABASE_URL absent; embed test skipped");
        return;
    };
    let store = Store::new(database.pool.clone());
    let principal = principal(&database.pool, "embed-user").await;
    let issued = store
        .issue_embed_code(
            &principal,
            IssueEmbedRequest {
                target_path: "/embed/actions/action-1".into(),
            },
            &policy(),
            &risk_facts(),
            Uuid::new_v4(),
        )
        .await
        .expect("issue embed code");
    assert_eq!(
        URL_SAFE_NO_PAD
            .decode(&issued.code)
            .expect("decode code")
            .len(),
        32
    );
    let ttl = issued.expires_at - chrono::Utc::now();
    assert!(ttl.num_seconds() >= 28 && ttl.num_seconds() <= 30);
    let row = sqlx::query(
        "SELECT code_hash,workbench_user_id,workbench_session_id,deployment_id,target_path,status
         FROM life_embed_codes WHERE id=$1",
    )
    .bind(issued.embed_session_id.as_uuid())
    .fetch_one(&database.pool)
    .await
    .expect("embed row");
    assert_eq!(
        row.get::<Vec<u8>, _>("code_hash"),
        Sha256::digest(issued.code.as_bytes()).to_vec()
    );
    assert_ne!(row.get::<Vec<u8>, _>("code_hash"), issued.code.as_bytes());
    assert_eq!(
        row.get::<Uuid, _>("workbench_user_id"),
        principal.user_id.as_uuid()
    );
    assert_eq!(
        row.get::<Uuid, _>("workbench_session_id"),
        principal.session_id.as_uuid()
    );
    assert_eq!(row.get::<String, _>("deployment_id"), "life-test");
    assert_eq!(
        row.get::<String, _>("target_path"),
        "/embed/actions/action-1"
    );
    assert!(store
        .consume_embed_code(
            &issued.code,
            "other-deployment",
            &policy(),
            &risk_facts(),
            Uuid::new_v4(),
        )
        .await
        .is_err());

    let barrier = Arc::new(Barrier::new(17));
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let store = store.clone();
        let code = issued.code.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            store
                .consume_embed_code(&code, "life-test", &policy(), &risk_facts(), Uuid::new_v4())
                .await
        }));
    }
    barrier.wait().await;
    let mut consumed = Vec::new();
    for task in tasks {
        if let Ok(value) = task.await.expect("join consumer") {
            consumed.push(value);
        }
    }
    assert_eq!(consumed.len(), 1);
    let consumed = consumed.pop().expect("one consumer");
    assert_eq!(consumed.embed_session_id, issued.embed_session_id);
    assert_eq!(consumed.target_path, "/embed/actions/action-1");
    assert_eq!(consumed.workbench_user_id, principal.user_id);
    assert_eq!(consumed.life_os_user_id, "life-embed-user");
    assert_eq!(consumed.workbench_session_id, principal.session_id);
    assert_eq!(consumed.deployment_id, "life-test");
    let session_hash: Vec<u8> =
        sqlx::query_scalar("SELECT session_token_hash FROM life_embed_sessions WHERE id=$1")
            .bind(consumed.embed_session_id.as_uuid())
            .fetch_one(&database.pool)
            .await
            .expect("session hash");
    assert_eq!(
        session_hash,
        Sha256::digest(consumed.session_token.as_bytes()).to_vec()
    );
    assert_ne!(session_hash, consumed.session_token.as_bytes());
    assert!(store
        .consume_embed_code(
            &issued.code,
            "life-test",
            &policy(),
            &risk_facts(),
            Uuid::new_v4(),
        )
        .await
        .is_err());
    store
        .revoke_embed_session(&principal, consumed.embed_session_id, Uuid::new_v4())
        .await
        .expect("revoke consumed session");
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM life_embed_sessions WHERE id=$1")
            .bind(consumed.embed_session_id.as_uuid())
            .fetch_one(&database.pool)
            .await
            .expect("revoked session status"),
        "revoked"
    );

    database.cleanup().await;
}

#[tokio::test]
async fn embed_target_expiry_ownership_and_revocation_fail_closed() {
    let Some(database) = Database::create().await else {
        return;
    };
    let store = Store::new(database.pool.clone());
    let owner = principal(&database.pool, "owner").await;
    let attacker = principal(&database.pool, "attacker").await;
    for path in [
        "/",
        "/embed/../admin",
        "/embed/actions/a/b",
        "/embed/actions/%2fadmin",
        "/embed/calendar?date=2026-9-1",
        "/embed/calendar?date=2026-09-01&admin=1",
        "https://evil.example/embed/dashboard",
    ] {
        assert!(
            store
                .issue_embed_code(
                    &owner,
                    IssueEmbedRequest {
                        target_path: path.into()
                    },
                    &policy(),
                    &risk_facts(),
                    Uuid::new_v4(),
                )
                .await
                .is_err(),
            "invalid target accepted: {path}"
        );
    }
    let revoked = store
        .issue_embed_code(
            &owner,
            IssueEmbedRequest {
                target_path: "/embed/dashboard".into(),
            },
            &policy(),
            &risk_facts(),
            Uuid::new_v4(),
        )
        .await
        .expect("issue revocable code");
    assert!(store
        .revoke_embed_session(&attacker, revoked.embed_session_id, Uuid::new_v4())
        .await
        .is_err());
    store
        .revoke_embed_session(&owner, revoked.embed_session_id, Uuid::new_v4())
        .await
        .expect("owner revoke");
    assert!(store
        .consume_embed_code(
            &revoked.code,
            "life-test",
            &policy(),
            &risk_facts(),
            Uuid::new_v4(),
        )
        .await
        .is_err());

    let expired = store
        .issue_embed_code(
            &owner,
            IssueEmbedRequest {
                target_path: "/embed/calendar?date=2026-09-01".into(),
            },
            &policy(),
            &risk_facts(),
            Uuid::new_v4(),
        )
        .await
        .expect("issue expiring code");
    sqlx::query(
        "UPDATE life_embed_codes
         SET created_at=now()-interval '1 minute',expires_at=now()-interval '1 second'
         WHERE id=$1",
    )
    .bind(expired.embed_session_id.as_uuid())
    .execute(&database.pool)
    .await
    .expect("expire code");
    assert!(store
        .consume_embed_code(
            &expired.code,
            "life-test",
            &policy(),
            &risk_facts(),
            Uuid::new_v4(),
        )
        .await
        .is_err());
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM life_embed_codes WHERE id=$1")
            .bind(expired.embed_session_id.as_uuid())
            .fetch_one(&database.pool)
            .await
            .expect("expired status"),
        "expired"
    );
    database.cleanup().await;
}
