use life_auth_gateway::{
    target_selection::{
        ConsumeTargetSelectionRequest, IssueTargetSelectionRequest, TargetSelectionKind,
    },
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
            .ok()?;
        let schema = format!("life_target_test_{}", Uuid::new_v4().simple());
        sqlx::raw_sql(AssertSqlSafe(format!("CREATE SCHEMA \"{schema}\"")))
            .execute(&admin)
            .await
            .expect("create isolated schema");
        let options = PgConnectOptions::from_str(&url)
            .expect("parse database URL")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
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

async fn seed_binding(pool: &PgPool, life_os_user_id: &str, pubkey: &str) {
    let user_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO life_workbench_users
         (id,oidc_issuer,oidc_subject,life_os_user_id,status)
         VALUES($1,'https://identity.example',$2,$3,'active')",
    )
    .bind(user_id)
    .bind(format!("subject-{life_os_user_id}"))
    .bind(life_os_user_id)
    .execute(pool)
    .await
    .expect("seed user");
    sqlx::query(
        "INSERT INTO life_identity_bindings
         (id,workbench_user_id,buzz_pubkey,source_event_id,status)
         VALUES($1,$2,$3,$4,'active')",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(pubkey)
    .bind("f".repeat(64))
    .execute(pool)
    .await
    .expect("seed binding");
}

fn issue(kind: TargetSelectionKind, channel_id: Option<&str>) -> IssueTargetSelectionRequest {
    IssueTargetSelectionRequest {
        kind,
        life_os_user_id: "life-user".into(),
        community_id: "community-1".into(),
        user_pubkey: "a".repeat(64),
        channel_id: channel_id.map(str::to_owned),
    }
}

#[tokio::test]
async fn target_selection_is_bound_single_use_and_rechecks_identity() {
    let Some(database) = TestDatabase::create().await else {
        eprintln!("LIFE_AUTH_TEST_DATABASE_URL absent; target selection test skipped");
        return;
    };
    seed_binding(&database.pool, "life-user", &"a".repeat(64)).await;
    let store = Store::new(database.pool.clone());

    let identity = store
        .issue_target_selection(issue(TargetSelectionKind::Identity, None), Uuid::new_v4())
        .await
        .expect("issue identity selection");
    let consumed = store
        .consume_target_selection(
            identity.selection_id,
            ConsumeTargetSelectionRequest {
                kind: TargetSelectionKind::Identity,
                life_os_user_id: "life-user".into(),
            },
        )
        .await
        .expect("consume identity selection");
    assert_eq!(consumed.community_id, "community-1");
    assert_eq!(consumed.user_pubkey, "a".repeat(64));
    assert!(store
        .consume_target_selection(
            identity.selection_id,
            ConsumeTargetSelectionRequest {
                kind: TargetSelectionKind::Identity,
                life_os_user_id: "life-user".into(),
            },
        )
        .await
        .is_err());

    let channel = store
        .issue_target_selection(
            issue(TargetSelectionKind::Channel, Some("channel-1")),
            Uuid::new_v4(),
        )
        .await
        .expect("issue channel selection");
    sqlx::query(
        "UPDATE life_identity_bindings SET status='revoked',revoked_at=now()
         WHERE buzz_pubkey=$1",
    )
    .bind("a".repeat(64))
    .execute(&database.pool)
    .await
    .expect("revoke identity");
    assert!(store
        .consume_target_selection(
            channel.selection_id,
            ConsumeTargetSelectionRequest {
                kind: TargetSelectionKind::Channel,
                life_os_user_id: "life-user".into(),
            },
        )
        .await
        .is_err());
    assert!(store
        .issue_target_selection(issue(TargetSelectionKind::Identity, None), Uuid::new_v4(),)
        .await
        .is_err());

    database.cleanup().await;
}
