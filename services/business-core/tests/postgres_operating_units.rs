use sqlx::{
    migrate::Migrator,
    postgres::{PgConnectOptions, PgPoolOptions},
    AssertSqlSafe,
};
use std::{path::Path, str::FromStr};
use uuid::Uuid;

#[tokio::test]
async fn migration_preserves_fact_dimensions_and_builds_one_tree() {
    let Ok(database_url) = std::env::var("BUSINESS_CORE_TEST_DATABASE_URL") else {
        eprintln!("skipping: BUSINESS_CORE_TEST_DATABASE_URL is not set");
        return;
    };
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .unwrap();
    let database_name = format!("bizos_tree_{}", Uuid::new_v4().simple());
    sqlx::query(AssertSqlSafe(format!(
        "CREATE DATABASE \"{database_name}\""
    )))
    .execute(&admin)
    .await
    .unwrap();
    let options = PgConnectOptions::from_str(&database_url)
        .unwrap()
        .database(&database_name);
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .unwrap();
    let migrations_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../business-auth-gateway/migrations");
    let mut baseline = Migrator::new(migrations_path.as_path()).await.unwrap();
    baseline
        .migrations
        .to_mut()
        .retain(|item| item.version <= 34);
    baseline.run(&pool).await.unwrap();

    let group_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let legal_one = Uuid::new_v4();
    let legal_two = Uuid::new_v4();
    let unit_one = Uuid::new_v4();
    let unit_two = Uuid::new_v4();
    let customer_one = Uuid::new_v4();
    let customer_two = Uuid::new_v4();
    let order_one = Uuid::new_v4();
    let order_two = Uuid::new_v4();

    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'https://identity.test','migration-user','Migration User')")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO business_group_profile(id,code,name,base_currency,timezone) VALUES($1,'MIGRATION_GROUP','Migration Group','CNY','Asia/Shanghai')")
        .bind(group_id)
        .execute(&pool)
        .await
        .unwrap();
    for (id, code, name) in [
        (legal_one, "LEGAL_ONE", "Legal One"),
        (legal_two, "LEGAL_TWO", "Legal Two"),
    ] {
        sqlx::query("INSERT INTO business_legal_entities(id,code,name,country_code,functional_currency) VALUES($1,$2,$3,'CN','CNY')")
            .bind(id)
            .bind(code)
            .bind(name)
            .execute(&pool)
            .await
            .unwrap();
    }
    for (id, legal_id, code, name) in [
        (unit_one, legal_one, "UNIT_ONE", "Unit One"),
        (unit_two, legal_two, "UNIT_TWO", "Unit Two"),
    ] {
        sqlx::query("INSERT INTO business_units(id,legal_entity_id,code,name) VALUES($1,$2,$3,$4)")
            .bind(id)
            .bind(legal_id)
            .bind(code)
            .bind(name)
            .execute(&pool)
            .await
            .unwrap();
    }
    for (id, legal_id, unit_id, code) in [
        (customer_one, legal_one, unit_one, "CUSTOMER_ONE"),
        (customer_two, legal_two, unit_two, "CUSTOMER_TWO"),
    ] {
        sqlx::query("INSERT INTO business_customers(id,legal_entity_id,business_unit_id,code,name,credit_currency) VALUES($1,$2,$3,$4,$4,'CNY')")
            .bind(id)
            .bind(legal_id)
            .bind(unit_id)
            .bind(code)
            .execute(&pool)
            .await
            .unwrap();
    }
    for (id, number, legal_id, customer_id, unit_id) in [
        (
            order_one,
            "SO-MIGRATION-1",
            legal_one,
            customer_one,
            unit_one,
        ),
        (
            order_two,
            "SO-MIGRATION-2",
            legal_two,
            customer_two,
            unit_two,
        ),
    ] {
        sqlx::query("INSERT INTO sales_orders(id,order_number,legal_entity_id,customer_id,salesperson_user_id,business_unit_id,currency,order_date,payment_terms_days,payment_terms_snapshot,subtotal_amount,discount_amount,net_amount,tax_amount,gross_amount,created_by_user_id,updated_by_user_id,trace_id) VALUES($1,$2,$3,$4,$5,$6,'CNY',CURRENT_DATE,30,'{}',1,0,1,0,1,$5,$5,$7)")
            .bind(id)
            .bind(number)
            .bind(legal_id)
            .bind(customer_id)
            .bind(user_id)
            .bind(unit_id)
            .bind(Uuid::new_v4())
            .execute(&pool)
            .await
            .unwrap();
    }

    Migrator::new(migrations_path.as_path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();

    for (order_id, legal_id, unit_id) in [
        (order_one, legal_one, unit_one),
        (order_two, legal_two, unit_two),
    ] {
        let fact = sqlx::query_as::<_, (Uuid, Uuid)>(
            "SELECT legal_entity_id,business_unit_id FROM sales_orders WHERE id=$1",
        )
        .bind(order_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(fact, (legal_id, unit_id));
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM business_units WHERE is_operating_root",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM business_units WHERE NOT is_operating_root AND parent_business_unit_id IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    let (walked, distinct): (i64, i64) = sqlx::query_as(
        "WITH RECURSIVE tree AS (SELECT id FROM business_units WHERE is_operating_root UNION ALL SELECT child.id FROM business_units child JOIN tree parent ON child.parent_business_unit_id=parent.id) SELECT count(*),count(DISTINCT id) FROM tree",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let total = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM business_units")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((walked, distinct), (total, total));

    pool.close().await;
    sqlx::query(AssertSqlSafe(format!(
        "DROP DATABASE \"{database_name}\" WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
}
