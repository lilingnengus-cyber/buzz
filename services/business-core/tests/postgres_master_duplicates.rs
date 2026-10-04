use business_core::{
    master_data::{CoreMasterDataService, SaveCoreMasterData},
    PgStore,
};
use sqlx::{
    migrate::Migrator,
    postgres::{PgConnectOptions, PgPoolOptions},
    AssertSqlSafe,
};
use std::{path::Path, str::FromStr};
use uuid::Uuid;

#[tokio::test]
async fn agent_duplicate_check_is_scoped_idempotent_and_serialized() {
    let Ok(url) = std::env::var("BUSINESS_CORE_TEST_DATABASE_URL") else {
        return;
    };
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let db = format!("master_duplicates_{}", Uuid::new_v4().simple());
    sqlx::query(AssertSqlSafe(format!("CREATE DATABASE \"{db}\"")))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(PgConnectOptions::from_str(&url).unwrap().database(&db))
        .await
        .unwrap();
    Migrator::new(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../business-auth-gateway/migrations")
            .as_path(),
    )
    .await
    .unwrap()
    .run(&pool)
    .await
    .unwrap();
    let actor = Uuid::new_v4();
    let role = Uuid::new_v4();
    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'https://test','pages','Pages')").bind(actor).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_roles(id,role_key,name) VALUES($1,'pages','Pages')")
        .bind(role)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,'business_master_data:read'),($1,'business_master_data:manage')").bind(role).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) VALUES($1,$2,$1)",
    )
    .bind(actor)
    .bind(role)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_customers(id,code,name,credit_currency,status) VALUES(gen_random_uuid(),'EXISTING','ＡＢＣ 公司','CNY','disabled'),(gen_random_uuid(),'HIDDEN','隐藏公司','CNY','active')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) SELECT $1,id,$1 FROM business_customers WHERE code='EXISTING'").bind(actor).execute(&pool).await.unwrap();
    let core = CoreMasterDataService::new(PgStore::new(pool.clone()));
    let input: SaveCoreMasterData = serde_json::from_value(serde_json::json!({"resourceType":"customer","code":"AUTO","name":"abc公司","creditCurrency":"CNY"})).unwrap();
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM business_customers")
        .fetch_one(&pool)
        .await
        .unwrap();
    let error = core
        .save_agent_customer(actor, Uuid::new_v4(), "duplicate", &input, false)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("DUPLICATE_CUSTOMER:")
            && error.contains("EXISTING")
            && error.contains("已停用")
    );
    assert!(!error.contains("HIDDEN"));
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM business_customers")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(before, after);
    let created = core
        .save_agent_customer(actor, Uuid::new_v4(), "duplicate", &input, true)
        .await
        .unwrap();
    let replay = core
        .save_agent_customer(actor, Uuid::new_v4(), "duplicate", &input, true)
        .await
        .unwrap();
    assert_eq!(created.id, replay.id);
    assert!(replay.idempotent_replay);
    let mut hidden = input.clone();
    hidden.name = "隐藏公司".into();
    core.save_agent_customer(actor, Uuid::new_v4(), "duplicate-hidden", &hidden, false)
        .await
        .unwrap();
    let mut concurrent = input.clone();
    concurrent.name = "并发测试公司".into();
    let (a, b) = tokio::join!(
        core.save_agent_customer(
            actor,
            Uuid::new_v4(),
            "duplicate-race-a",
            &concurrent,
            false
        ),
        core.save_agent_customer(
            actor,
            Uuid::new_v4(),
            "duplicate-race-b",
            &concurrent,
            false
        )
    );
    assert_ne!(a.is_ok(), b.is_ok());
    pool.close().await;
    sqlx::query(AssertSqlSafe(format!(
        "DROP DATABASE \"{db}\" WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    admin.close().await;
}
