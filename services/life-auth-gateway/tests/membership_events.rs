use life_auth_gateway::{
    membership::{MembershipEvent, MembershipSnapshot},
    Store,
};
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

async fn seed_user_and_dependants(pool: &PgPool) -> Uuid {
    let user_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO life_workbench_users
         (id,oidc_issuer,oidc_subject,life_os_user_id,status)
         VALUES($1,'https://identity.example','subject','life-user','active')",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .expect("seed user");
    sqlx::query(
        "INSERT INTO life_workbench_sessions
         (id,workbench_user_id,deployment_id,token_hash,status,expires_at)
         VALUES($1,$2,'life-test',$3,'active',now()+interval '1 hour')",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(vec![41_u8; 32])
    .execute(pool)
    .await
    .expect("seed session");
    sqlx::query(
        "INSERT INTO life_agent_delegations
         (id,token_hash,workbench_user_id,workbench_session_id,agent_id,agent_turn_id,
          source_event_id,source_pubkey,audience,capabilities,data_scope,obligations,status,
          expires_at,max_calls,remaining_calls,trace_id)
         VALUES($1,$2,$3,$4,'agent','turn',$5,$6,'life-workbench-mcp','[]','{}','[]',
                'active',now()+interval '5 minutes',1,1,$7)",
    )
    .bind(Uuid::new_v4())
    .bind(vec![42_u8; 32])
    .bind(user_id)
    .bind(session_id)
    .bind("a".repeat(64))
    .bind("b".repeat(64))
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .expect("seed delegation");
    sqlx::query(
        "INSERT INTO life_embed_sessions
         (id,session_token_hash,workbench_user_id,workbench_session_id,deployment_id,
          status,expires_at,trace_id)
         VALUES($1,$2,$3,$4,'life-test','active',now()+interval '5 minutes',$5)",
    )
    .bind(Uuid::new_v4())
    .bind(vec![43_u8; 32])
    .bind(user_id)
    .bind(session_id)
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .expect("seed embed session");
    user_id
}

fn event(version: i64, role: &str, user_active: bool) -> MembershipEvent {
    MembershipEvent {
        life_os_user_id: "life-user".into(),
        user_active,
        membership_version: version,
        memberships: if role.is_empty() {
            vec![]
        } else {
            vec![MembershipSnapshot {
                workspace_id: "workspace-1".into(),
                role: role.into(),
            }]
        },
        trace_id: Uuid::new_v4(),
    }
}

#[tokio::test]
async fn membership_snapshots_are_monotonic_idempotent_and_revoke_authority() {
    let Some(database) = TestDatabase::create().await else {
        eprintln!("LIFE_AUTH_TEST_DATABASE_URL absent; membership integration test skipped");
        return;
    };
    seed_user_and_dependants(&database.pool).await;
    let store = Store::new(database.pool.clone());

    assert!(store
        .apply_membership_event(&event(7, "OWNER", true))
        .await
        .expect("apply current snapshot"));
    assert!(!store
        .apply_membership_event(&event(7, "VIEWER", true))
        .await
        .expect("ignore duplicate snapshot"));
    assert!(!store
        .apply_membership_event(&event(6, "VIEWER", true))
        .await
        .expect("ignore out-of-order snapshot"));
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT role_code FROM life_workspace_memberships WHERE status='active'",
        )
        .fetch_one(&database.pool)
        .await
        .expect("current role"),
        "OWNER"
    );

    assert!(store
        .apply_membership_event(&event(8, "", false))
        .await
        .expect("apply user disable snapshot"));
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT status FROM life_workbench_users WHERE life_os_user_id='life-user'",
        )
        .fetch_one(&database.pool)
        .await
        .expect("user status"),
        "disabled"
    );
    let active_authority = sqlx::query_scalar::<_, i64>(
        "SELECT
          (SELECT count(*) FROM life_workspace_memberships WHERE status='active') +
          (SELECT count(*) FROM life_agent_delegations WHERE status='active') +
          (SELECT count(*) FROM life_embed_sessions WHERE status='active')",
    )
    .fetch_one(&database.pool)
    .await
    .expect("active authority count");
    assert_eq!(active_authority, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM life_security_audit
             WHERE event_type='MEMBERSHIP_SNAPSHOT_APPLIED' AND outcome='success'",
        )
        .fetch_one(&database.pool)
        .await
        .expect("membership audit count"),
        2
    );

    database.cleanup().await;
}
