use business_core::{
    master_data::SaveCoreMasterData,
    operating_units::{descendant_ids, has_active_descendants, validate_parent},
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
async fn operating_unit_tree_rejects_cycles_and_disabled_parents() {
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
    Migrator::new(migrations_path.as_path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();

    let legal_id = Uuid::new_v4();
    let root = Uuid::new_v4();
    let division = Uuid::new_v4();
    let team = Uuid::new_v4();
    let leaf = Uuid::new_v4();
    sqlx::query("INSERT INTO business_group_profile(id,code,name,base_currency,timezone) VALUES($1,'TREE_GROUP','Tree Group','CNY','Asia/Shanghai')")
        .bind(Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO business_legal_entities(id,code,name,country_code,functional_currency) VALUES($1,'TREE_LEGAL','Tree Legal','CN','CNY')")
        .bind(legal_id)
        .execute(&pool)
        .await
        .unwrap();
    for (id, parent, code, is_root) in [
        (root, None, "TREE_ROOT", true),
        (division, Some(root), "TREE_DIVISION", false),
        (team, Some(division), "TREE_TEAM", false),
        (leaf, Some(team), "TREE_LEAF", false),
    ] {
        sqlx::query("INSERT INTO business_units(id,legal_entity_id,parent_business_unit_id,is_operating_root,code,name) VALUES($1,$2,$3,$4,$5,$5)")
            .bind(id)
            .bind(legal_id)
            .bind(parent)
            .bind(is_root)
            .bind(code)
            .execute(&pool)
            .await
            .unwrap();
    }

    let parsed: SaveCoreMasterData = serde_json::from_value(serde_json::json!({
        "resourceType":"business_unit",
        "code":"TREE_CHILD",
        "name":"Tree Child",
        "parentBusinessUnitId":leaf
    }))
    .unwrap();
    assert_eq!(parsed.parent_business_unit_id, Some(leaf));

    let mut tx = pool.begin().await.unwrap();
    validate_parent(&mut tx, Uuid::new_v4(), Some(leaf))
        .await
        .unwrap();
    assert_eq!(
        validate_parent(&mut tx, leaf, Some(leaf))
            .await
            .unwrap_err()
            .to_string(),
        "invalid input: OPERATING_UNIT_CYCLE"
    );
    assert_eq!(
        validate_parent(&mut tx, root, Some(leaf))
            .await
            .unwrap_err()
            .to_string(),
        "invalid input: OPERATING_UNIT_CYCLE"
    );
    sqlx::query("UPDATE business_units SET status='disabled' WHERE id=$1")
        .bind(team)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        validate_parent(&mut tx, Uuid::new_v4(), Some(team))
            .await
            .unwrap_err()
            .to_string(),
        "not found or forbidden"
    );
    tx.rollback().await.unwrap();

    let roots = [division].into_iter().collect();
    assert_eq!(
        descendant_ids(&pool, &roots, true).await.unwrap(),
        [division, team, leaf].into_iter().collect()
    );
    assert!(has_active_descendants(&pool, division).await.unwrap());

    pool.close().await;
    sqlx::query(AssertSqlSafe(format!(
        "DROP DATABASE \"{database_name}\" WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
}

#[tokio::test]
async fn operating_unit_scope_includes_descendants_without_siblings_or_duplicates() {
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
    Migrator::new(migrations_path.as_path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();

    let user = Uuid::new_v4();
    let legal = Uuid::new_v4();
    let root = Uuid::new_v4();
    let north = Uuid::new_v4();
    let hangzhou = Uuid::new_v4();
    let south = Uuid::new_v4();
    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'https://identity.test','scope-user','Scope User')")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO business_group_profile(id,code,name,base_currency,timezone) VALUES($1,'SCOPE_GROUP','Scope Group','CNY','Asia/Shanghai')")
        .bind(Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO business_legal_entities(id,code,name,country_code,functional_currency) VALUES($1,'SCOPE_LEGAL','Scope Legal','CN','CNY')")
        .bind(legal)
        .execute(&pool)
        .await
        .unwrap();
    for (id, parent, code, is_root) in [
        (root, None, "SCOPE_ROOT", true),
        (north, Some(root), "SCOPE_NORTH", false),
        (hangzhou, Some(north), "SCOPE_HANGZHOU", false),
        (south, Some(root), "SCOPE_SOUTH", false),
    ] {
        sqlx::query("INSERT INTO business_units(id,legal_entity_id,parent_business_unit_id,is_operating_root,code,name) VALUES($1,$2,$3,$4,$5,$5)")
            .bind(id)
            .bind(legal)
            .bind(parent)
            .bind(is_root)
            .bind(code)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)")
        .bind(user)
        .bind(north)
        .execute(&pool)
        .await
        .unwrap();

    let store = PgStore::new(pool.clone());
    let snapshot = store.snapshot(user).await.unwrap();
    assert_eq!(
        snapshot.scopes.business_unit_ids,
        [north, hangzhou].into_iter().collect()
    );
    assert!(!snapshot.scopes.business_unit_ids.contains(&south));

    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)")
        .bind(user)
        .bind(root)
        .execute(&pool)
        .await
        .unwrap();
    let overlapping = store.snapshot(user).await.unwrap();
    assert_eq!(overlapping.scopes.business_unit_ids.len(), 4);
    assert_eq!(
        overlapping.scopes.business_unit_ids,
        [root, north, hangzhou, south].into_iter().collect()
    );

    sqlx::query("UPDATE business_units SET status='disabled' WHERE id=$1")
        .bind(hangzhou)
        .execute(&pool)
        .await
        .unwrap();
    assert!(store
        .snapshot(user)
        .await
        .unwrap()
        .scopes
        .business_unit_ids
        .contains(&hangzhou));
    assert!(
        !descendant_ids(&pool, &[north].into_iter().collect(), false)
            .await
            .unwrap()
            .contains(&hangzhou)
    );

    pool.close().await;
    sqlx::query(AssertSqlSafe(format!(
        "DROP DATABASE \"{database_name}\" WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
}

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
