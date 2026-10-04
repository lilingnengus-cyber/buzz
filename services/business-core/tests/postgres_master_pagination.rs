use business_core::{
    master_data::{CoreMasterDataService, CoreMasterType},
    master_pagination::MasterPageFilter,
    product_master::{ProductMasterService, ProductMasterType},
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
async fn pages_search_and_counts_remain_complete_and_permission_scoped() {
    let Ok(url) = std::env::var("BUSINESS_CORE_TEST_DATABASE_URL") else {
        return;
    };
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let db = format!("master_pages_{}", Uuid::new_v4().simple());
    sqlx::query(AssertSqlSafe(format!("CREATE DATABASE \"{db}\"")))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(2)
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
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,'business_master_data:read'),($1,'business_product_master:read')").bind(role).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) VALUES($1,$2,$1)",
    )
    .bind(actor)
    .bind(role)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_customers(id,code,name,credit_currency,status) SELECT gen_random_uuid(),'CU-'||lpad(n::text,5,'0'),CASE WHEN n=1005 THEN 'Target %_客户' ELSE 'Customer '||n END,'CNY',CASE WHEN n=1005 THEN 'disabled' ELSE 'active' END FROM generate_series(1,1006) n").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) SELECT $1,id,$1 FROM business_customers WHERE code<>'CU-01006'").bind(actor).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_brands(id,code,name) SELECT gen_random_uuid(),'BR-'||lpad(n::text,5,'0'),CASE WHEN n=2005 THEN 'Target %_品牌' ELSE 'Brand '||n END FROM generate_series(1,2006) n").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) SELECT $1,id,$1 FROM business_brands WHERE code<>'BR-02006'").bind(actor).execute(&pool).await.unwrap();
    let core = CoreMasterDataService::new(PgStore::new(pool.clone()));
    let product = ProductMasterService::new(PgStore::new(pool.clone()));
    let first = core
        .list_page(
            actor,
            Some(CoreMasterType::Customer),
            1000,
            &MasterPageFilter::default(),
        )
        .await
        .unwrap();
    assert_eq!(first.items.len(), 1000);
    assert_eq!(first.page.total, 1005);
    assert!(first.page.has_more);
    assert_eq!(first.page.counts.get("customer"), Some(&1005));
    let last = core
        .list_page(
            actor,
            Some(CoreMasterType::Customer),
            1000,
            &MasterPageFilter {
                offset: 1000,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(last.items.len(), 5);
    assert!(!last.page.has_more);
    assert_eq!(last.items[4].code, "CU-01005");
    let search = core
        .list_page(
            actor,
            Some(CoreMasterType::Customer),
            50,
            &MasterPageFilter {
                query: "%_".into(),
                status: Some("disabled".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(search.page.total, 1);
    assert_eq!(search.items[0].code, "CU-01005");
    let hidden: Uuid =
        sqlx::query_scalar("SELECT id FROM business_customers WHERE code='CU-01006'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let denied = core
        .list_page(
            actor,
            None,
            50,
            &MasterPageFilter {
                id: Some(hidden),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(denied.page.total, 0);
    assert!(denied.items.is_empty());
    let first = product
        .list_page(
            actor,
            Some(ProductMasterType::Brand),
            2000,
            &MasterPageFilter::default(),
        )
        .await
        .unwrap();
    assert_eq!(first.page.total, 2005);
    assert_eq!(first.items.len(), 2000);
    assert!(first.page.has_more);
    assert_eq!(first.page.counts.get("brand"), Some(&2005));
    let last = product
        .list_page(
            actor,
            Some(ProductMasterType::Brand),
            2000,
            &MasterPageFilter {
                offset: 2000,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(last.items.len(), 5);
    assert!(!last.page.has_more);
    assert_eq!(last.items[4].code, "BR-02005");
    let search = product
        .list_page(
            actor,
            Some(ProductMasterType::Brand),
            50,
            &MasterPageFilter {
                query: "target %_".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(search.page.total, 1);
    assert_eq!(search.items[0].code, "BR-02005");
    assert!(core
        .list_page(
            actor,
            None,
            50,
            &MasterPageFilter {
                offset: -1,
                ..Default::default()
            }
        )
        .await
        .is_err());
    assert!(product
        .list_page(
            actor,
            None,
            50,
            &MasterPageFilter {
                status: Some("bogus".into()),
                ..Default::default()
            }
        )
        .await
        .is_err());
    pool.close().await;
    sqlx::query(AssertSqlSafe(format!(
        "DROP DATABASE \"{db}\" WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
}
