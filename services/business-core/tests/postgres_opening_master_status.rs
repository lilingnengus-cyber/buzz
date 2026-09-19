//! Order validation must observe status changes after waiting for a master row.
#[path = "support/b2_seed.rs"]
mod b2_seed;
use business_core::{
    b2::{
        model::{CreateInventoryOpening, DecimalString, InventoryOpeningLineInput, VersionCommand},
        InventoryService,
    },
    PgStore,
};
use chrono::NaiveDate;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;
fn dec(value: i64) -> DecimalString {
    DecimalString(rust_decimal::Decimal::from(value))
}
#[derive(Clone)]
struct Fixture {
    actor: Uuid,
    legal_entity: Uuid,
    business_unit: Uuid,
    warehouse: Uuid,
    customer: Uuid,
    brand: Uuid,
    uom: Uuid,
    sku: Uuid,
}

#[tokio::test]
async fn opening_create_and_post_recheck_locked_masters() {
    let Ok(url) = std::env::var("BUSINESS_OPENING_MASTER_TEST_DATABASE_URL") else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(6)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let f = b2_seed::seed(&pool).await;
    let product: Uuid = sqlx::query_scalar("SELECT product_id FROM business_skus WHERE id=$1")
        .bind(f.sku)
        .fetch_one(&pool)
        .await
        .unwrap();
    let category: Uuid =
        sqlx::query_scalar("SELECT category_id FROM business_products WHERE id=$1")
            .bind(product)
            .fetch_one(&pool)
            .await
            .unwrap();
    let config = business_core::Config::from_env().unwrap();
    let credential = config.service_credential.clone();
    let router = business_core::router(business_core::AppState::new(store.clone(), &config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let service = InventoryService::new(store, "OPEN".into(), "AR".into());
    let input = CreateInventoryOpening {
        legal_entity_id: f.legal_entity,
        business_date: NaiveDate::from_ymd_opt(2026, 9, 20).unwrap(),
        currency: "CNY".into(),
        lines: vec![InventoryOpeningLineInput {
            warehouse_id: f.warehouse,
            sku_id: f.sku,
            quantity: dec(1),
            unit_cost: dec(10),
        }],
    };
    let targets = [
        ("business_warehouses", f.warehouse),
        ("business_skus", f.sku),
        ("business_products", product),
        ("business_legal_entities", f.legal_entity),
        ("business_units", f.business_unit),
        ("business_units_of_measure", f.uom),
        ("business_product_categories", category),
        ("business_brands", f.brand),
    ];
    let mut batch = None;
    for posting in [false, true] {
        if posting {
            batch = Some(
                service
                    .create_opening(f.actor, Uuid::new_v4(), "positive-opening", &input)
                    .await
                    .unwrap()
                    .id,
            );
        }
        for (table, id) in targets {
            let mut tx = pool.begin().await.unwrap();
            let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "SELECT id FROM {table} WHERE id=$1 FOR UPDATE"
            )))
            .bind(id)
            .execute(&mut *tx)
            .await
            .unwrap();
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {table} SET status='disabled' WHERE id=$1"
            )))
            .bind(id)
            .execute(&mut *tx)
            .await
            .unwrap();
            let task_service = service.clone();
            let task_input = input.clone();
            let actor = f.actor;
            let request = tokio::spawn(async move {
                let key = format!("opening-{posting}-{table}");
                if let Some(batch) = batch {
                    task_service
                        .post_opening(
                            actor,
                            Uuid::new_v4(),
                            batch,
                            &key,
                            &VersionCommand {
                                expected_version: 1,
                                reason_code: None,
                            },
                        )
                        .await
                } else {
                    task_service
                        .create_opening(actor, Uuid::new_v4(), &key, &task_input)
                        .await
                }
            });
            blocked_pid(&pool, pid).await;
            tx.commit().await.unwrap();
            let result = request.await.unwrap();
            assert!(
                matches!(
                    result,
                    Err(business_core::b2::DomainError::NotFoundOrForbidden)
                ),
                "{posting}/{table}: {result:?}"
            );
            if let Some(batch) = batch {
                let preview: serde_json::Value = reqwest::Client::new()
                    .get(format!("http://{address}/v1/agent-approval-previews/stock/inventory_opening/{batch}"))
                    .header("x-business-service-credential",&credential)
                    .header("x-service-audience","business-core")
                    .header("x-enterprise-user-id",f.actor.to_string())
                    .header("x-trace-id",Uuid::new_v4().to_string())
                    .send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
                assert_eq!(preview["item"]["canConfirm"], false, "{table}");
                assert_eq!(preview["item"]["lines"][0]["ready"], false, "{table}");
            }
            let movements: i64 = sqlx::query_scalar("SELECT count(*) FROM inventory_movements")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(movements, 0);
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {table} SET status='active' WHERE id=$1"
            )))
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        }
    }
    let preview: serde_json::Value = reqwest::Client::new()
        .get(format!(
            "http://{address}/v1/agent-approval-previews/stock/inventory_opening/{}",
            batch.unwrap()
        ))
        .header("x-business-service-credential", &credential)
        .header("x-service-audience", "business-core")
        .header("x-enterprise-user-id", f.actor.to_string())
        .header("x-trace-id", Uuid::new_v4().to_string())
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(preview["item"]["canConfirm"], true);
    assert_eq!(preview["item"]["lines"][0]["ready"], true);
    let result = service
        .post_opening(
            f.actor,
            Uuid::new_v4(),
            batch.unwrap(),
            "positive-posting",
            &VersionCommand {
                expected_version: 1,
                reason_code: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.status, "posted");
    server.abort();
}
async fn blocked_pid(pool: &sqlx::PgPool, blocker: i32) -> i32 {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let pid: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(blocker)
            .fetch_optional(pool)
            .await
            .unwrap();
            if let Some(pid) = pid {
                return pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("expected real database lock wait")
}
