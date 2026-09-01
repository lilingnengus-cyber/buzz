use life_auth_gateway::{
    model::{AgentDelegationId, IdentityBindingChallengeId, LifeWorkbenchUserId},
    Store,
};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    AssertSqlSafe, PgPool, Row,
};
use std::str::FromStr;
use uuid::Uuid;

struct TestDatabase {
    admin: PgPool,
    pool: PgPool,
    schema: String,
}

impl TestDatabase {
    async fn create() -> Self {
        let url = std::env::var("LIFE_AUTH_TEST_DATABASE_URL")
            .expect("LIFE_AUTH_TEST_DATABASE_URL must name an isolated PostgreSQL database");
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
            .max_connections(4)
            .connect_with(options)
            .await
            .expect("connect isolated schema");
        Store::migrate(&pool).await.expect("run Life migrations");
        Self {
            admin,
            pool,
            schema,
        }
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

async fn insert_user(pool: &PgPool, life_os_user_id: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO life_workbench_users
         (id,oidc_issuer,oidc_subject,life_os_user_id,status)
         VALUES($1,'https://identity.example','subject-' || $2,$2,'active')",
    )
    .bind(id)
    .bind(life_os_user_id)
    .execute(pool)
    .await
    .expect("insert user");
    id
}

#[tokio::test]
async fn postgres_security_contract_is_enforced() {
    let database = TestDatabase::create().await;
    let pool = &database.pool;
    let store = Store::new(pool.clone());

    let required_tables = [
        "life_workbench_users",
        "life_identity_binding_challenges",
        "life_identity_bindings",
        "life_workbench_sessions",
        "life_workspace_memberships",
        "life_principals",
        "life_principal_capabilities",
        "life_principal_data_scopes",
        "life_capability_catalog",
        "life_iam_decisions",
        "life_agent_delegations",
        "life_delegation_calls",
        "life_embed_codes",
        "life_embed_sessions",
        "life_write_command_confirmations",
        "life_security_audit",
    ];
    let tables = sqlx::query_scalar::<_, String>(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema=current_schema() ORDER BY table_name",
    )
    .fetch_all(pool)
    .await
    .expect("list tables");
    for table in required_tables {
        assert!(
            tables.iter().any(|actual| actual == table),
            "missing {table}"
        );
    }
    let invalid_timestamp_columns = sqlx::query(
        "SELECT table_name,column_name,data_type FROM information_schema.columns
         WHERE table_schema=current_schema() AND column_name LIKE '%\\_at' ESCAPE '\\'
           AND data_type<>'timestamp with time zone'",
    )
    .fetch_all(pool)
    .await
    .expect("inspect timestamp columns");
    assert!(invalid_timestamp_columns.is_empty());
    let secret_columns = sqlx::query(
        "SELECT table_name,column_name,data_type FROM information_schema.columns
         WHERE table_schema=current_schema()
           AND (column_name LIKE '%token%' OR column_name IN ('code','code_hash','nonce','nonce_hash'))",
    )
    .fetch_all(pool)
    .await
    .expect("inspect secret columns");
    assert!(!secret_columns.is_empty());
    for column in secret_columns {
        let name = column.get::<String, _>("column_name");
        assert!(name.ends_with("_hash"), "raw secret column: {name}");
        assert_eq!(column.get::<String, _>("data_type"), "bytea");
    }

    let user_a = insert_user(pool, "life-user-a").await;
    let user_b = insert_user(pool, "life-user-b").await;
    let workbench_session = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO life_workbench_sessions
         (id,workbench_user_id,deployment_id,token_hash,status,expires_at)
         VALUES($1,$2,'life-prod-cn',$3,'active',now()+interval '1 hour')",
    )
    .bind(workbench_session)
    .bind(user_a)
    .bind(vec![8_u8; 32])
    .execute(pool)
    .await
    .expect("insert bound Workbench session");
    let pubkey = "a".repeat(64);
    sqlx::query(
        "INSERT INTO life_identity_bindings
         (id,workbench_user_id,buzz_pubkey,source_event_id,status)
         VALUES($1,$2,$3,$4,'active')",
    )
    .bind(Uuid::new_v4())
    .bind(user_a)
    .bind(&pubkey)
    .bind("c".repeat(64))
    .execute(pool)
    .await
    .expect("first active binding");
    assert!(sqlx::query(
        "INSERT INTO life_identity_bindings
         (id,workbench_user_id,buzz_pubkey,source_event_id,status)
         VALUES($1,$2,$3,$4,'active')",
    )
    .bind(Uuid::new_v4())
    .bind(user_b)
    .bind(&pubkey)
    .bind("e".repeat(64))
    .execute(pool)
    .await
    .is_err());
    sqlx::query(
        "UPDATE life_identity_bindings SET status='revoked',revoked_at=now() WHERE buzz_pubkey=$1",
    )
    .bind(&pubkey)
    .execute(pool)
    .await
    .expect("revoke first binding");
    sqlx::query(
        "INSERT INTO life_identity_bindings
         (id,workbench_user_id,buzz_pubkey,source_event_id,status)
         VALUES($1,$2,$3,$4,'active')",
    )
    .bind(Uuid::new_v4())
    .bind(user_b)
    .bind(&pubkey)
    .bind("d".repeat(64))
    .execute(pool)
    .await
    .expect("partial binding index releases revoked pubkey");

    let challenge_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO life_identity_binding_challenges
         (id,workbench_user_id,workbench_session_id,deployment_id,buzz_pubkey,
          nonce_hash,status,expires_at)
         VALUES($1,$2,$3,'life-prod-cn',$4,$5,'active',now()+interval '5 minutes')",
    )
    .bind(challenge_id)
    .bind(user_a)
    .bind(workbench_session)
    .bind("b".repeat(64))
    .bind(vec![7_u8; 32])
    .execute(pool)
    .await
    .expect("insert challenge");
    assert!(store
        .consume_identity_binding_challenge(
            IdentityBindingChallengeId::new(challenge_id),
            LifeWorkbenchUserId::new(user_a),
        )
        .await
        .expect("consume challenge"));
    assert!(!store
        .consume_identity_binding_challenge(
            IdentityBindingChallengeId::new(challenge_id),
            LifeWorkbenchUserId::new(user_a),
        )
        .await
        .expect("replay challenge"));

    assert!(sqlx::query(
        "INSERT INTO life_workbench_sessions
         (id,workbench_user_id,deployment_id,token_hash,status,expires_at)
         VALUES($1,$2,'life-prod-cn',$3,'active',now()+interval '1 hour')",
    )
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(vec![9_u8; 32])
    .execute(pool)
    .await
    .is_err());
    assert!(sqlx::query(
        "INSERT INTO life_workbench_sessions
         (id,workbench_user_id,deployment_id,token_hash,status,expires_at)
         VALUES($1,$2,'',$3,'active',now()+interval '1 hour')",
    )
    .bind(Uuid::new_v4())
    .bind(user_a)
    .bind(vec![12_u8; 32])
    .execute(pool)
    .await
    .is_err());

    sqlx::query(
        "INSERT INTO life_capability_catalog
         (capability,allowed_tools,risk_class,requires_expected_version,
          default_max_calls,max_batch_size,obligations,catalog_version,status)
         VALUES('action:update',$1,'high',true,4,10,$2,7,'retired')",
    )
    .bind(serde_json::json!(["update_action"]))
    .bind(serde_json::json!(["human_confirmation"]))
    .execute(pool)
    .await
    .expect("insert catalog entry");
    let catalog = sqlx::query(
        "SELECT allowed_tools,risk_class,requires_expected_version,
                default_max_calls,max_batch_size,obligations,catalog_version
         FROM life_capability_catalog
         WHERE capability='action:update' AND catalog_version=7",
    )
    .fetch_one(pool)
    .await
    .expect("read catalog entry");
    assert_eq!(catalog.get::<String, _>("risk_class"), "high");
    assert!(catalog.get::<bool, _>("requires_expected_version"));
    assert_eq!(catalog.get::<i32, _>("default_max_calls"), 4);
    assert_eq!(catalog.get::<i32, _>("max_batch_size"), 10);
    assert_eq!(catalog.get::<i32, _>("catalog_version"), 7);
    assert_eq!(
        catalog.get::<serde_json::Value, _>("allowed_tools"),
        serde_json::json!(["update_action"])
    );
    assert_eq!(
        catalog.get::<serde_json::Value, _>("obligations"),
        serde_json::json!(["human_confirmation"])
    );

    let delegation = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO life_agent_delegations
         (id,token_hash,workbench_user_id,workbench_session_id,agent_id,
          agent_turn_id,source_event_id,source_pubkey,audience,capabilities,
          data_scope,obligations,status,expires_at,max_calls,remaining_calls,trace_id)
         VALUES($1,$2,$3,$4,'agent-1','turn-1',$5,$6,'life-workbench-mcp',
                '[]'::jsonb,'{}'::jsonb,'[]'::jsonb,'active',
                now()+interval '5 minutes',3,3,$7)",
    )
    .bind(delegation)
    .bind(vec![10_u8; 32])
    .bind(user_a)
    .bind(workbench_session)
    .bind("c".repeat(64))
    .bind(&pubkey)
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .expect("insert delegation");
    assert!(sqlx::query(
        "INSERT INTO life_agent_delegations
         (id,token_hash,workbench_user_id,workbench_session_id,agent_id,
          agent_turn_id,source_event_id,source_pubkey,audience,capabilities,
          data_scope,obligations,status,expires_at,max_calls,remaining_calls,trace_id)
         VALUES($1,$2,$3,$4,'agent-1','turn-1',$5,$6,'life-workbench-mcp',
                '[]'::jsonb,'{}'::jsonb,'[]'::jsonb,'active',
                now()+interval '5 minutes',3,3,$7)",
    )
    .bind(Uuid::new_v4())
    .bind(vec![11_u8; 32])
    .bind(user_a)
    .bind(workbench_session)
    .bind("c".repeat(64))
    .bind(&pubkey)
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .is_err());
    assert!(store
        .revoke_agent_delegation(AgentDelegationId::new(delegation))
        .await
        .expect("revoke delegation"));
    assert!(!store
        .revoke_agent_delegation(AgentDelegationId::new(delegation))
        .await
        .expect("repeat delegation revoke"));
    sqlx::query(
        "INSERT INTO life_agent_delegations
         (id,token_hash,workbench_user_id,workbench_session_id,agent_id,
          agent_turn_id,source_event_id,source_pubkey,audience,capabilities,
          data_scope,obligations,status,expires_at,max_calls,remaining_calls,trace_id)
         VALUES($1,$2,$3,$4,'agent-1','turn-1',$5,$6,'life-workbench-mcp',
                '[]'::jsonb,'{}'::jsonb,'[]'::jsonb,'active',
                now()+interval '5 minutes',3,3,$7)",
    )
    .bind(Uuid::new_v4())
    .bind(vec![11_u8; 32])
    .bind(user_a)
    .bind(workbench_session)
    .bind("f".repeat(64))
    .bind(&pubkey)
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .expect("partial delegation index releases revoked turn");
    let token_columns = sqlx::query(
        "SELECT column_name,data_type FROM information_schema.columns
         WHERE table_schema=current_schema()
           AND table_name='life_agent_delegations'
           AND column_name LIKE '%token%'",
    )
    .fetch_all(pool)
    .await
    .expect("delegation token columns");
    assert_eq!(token_columns.len(), 1);
    assert_eq!(
        token_columns[0].get::<String, _>("column_name"),
        "token_hash"
    );
    assert_eq!(token_columns[0].get::<String, _>("data_type"), "bytea");

    let audit_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO life_security_audit(event_type,outcome,trace_id)
         VALUES('SCHEMA_TEST','success',$1) RETURNING id",
    )
    .bind(Uuid::new_v4())
    .fetch_one(pool)
    .await
    .expect("append audit");
    assert!(
        sqlx::query("UPDATE life_security_audit SET outcome='changed' WHERE id=$1")
            .bind(audit_id)
            .execute(pool)
            .await
            .is_err()
    );
    assert!(sqlx::query("DELETE FROM life_security_audit WHERE id=$1")
        .bind(audit_id)
        .execute(pool)
        .await
        .is_err());
    let unsafe_audit_columns = sqlx::query_scalar::<_, String>(
        "SELECT column_name FROM information_schema.columns
         WHERE table_schema=current_schema() AND table_name='life_security_audit'
           AND (column_name LIKE '%body%' OR column_name LIKE '%content%'
                OR column_name LIKE '%payload%' OR column_name LIKE '%token%'
                OR column_name LIKE '%secret%')",
    )
    .fetch_all(pool)
    .await
    .expect("inspect audit columns");
    assert!(unsafe_audit_columns.is_empty());

    database.cleanup().await;
}

#[test]
fn migrations_do_not_reference_business_domain() {
    let migrations = concat!(
        include_str!("../migrations/0001_life_identity.sql"),
        include_str!("../migrations/0002_life_iam.sql"),
        include_str!("../migrations/0003_life_delegations.sql"),
        include_str!("../migrations/0004_life_embed_and_commands.sql"),
        include_str!("../migrations/0005_life_identity_runtime.sql"),
        include_str!("../migrations/0006_life_delegation_runtime.sql"),
    )
    .to_ascii_lowercase();
    assert!(!migrations.contains("business"));
    assert!(!migrations.contains("hermes"));
}
