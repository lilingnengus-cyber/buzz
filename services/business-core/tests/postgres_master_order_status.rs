//! Order validation must observe status changes after waiting for a master row.
#[path = "support/b2_seed.rs"]
mod b2_seed;
use business_core::{
    b2::{
        model::{CreateSalesOrder, DecimalString, SalesOrderLineInput},
        SalesService,
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
async fn sales_create_rechecks_master_status_after_disable_wait() {
    let Ok(url) = std::env::var("BUSINESS_MASTER_ORDER_TEST_DATABASE_URL") else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(6)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let fixture = b2_seed::seed(&pool).await;
    let customer = fixture.customer;
    let product: Uuid = sqlx::query_scalar("SELECT product_id FROM business_skus WHERE id=$1")
        .bind(fixture.sku)
        .fetch_one(&pool)
        .await
        .unwrap();
    for (table, id) in [
        ("business_customers", customer),
        ("business_units", fixture.business_unit),
        ("business_warehouses", fixture.warehouse),
        ("business_skus", fixture.sku),
        ("business_products", product),
    ] {
        let mut blocker = pool.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *blocker)
            .await
            .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT id FROM {table} WHERE id=$1 FOR UPDATE"
        )))
        .bind(id)
        .execute(&mut *blocker)
        .await
        .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET status='disabled' WHERE id=$1"
        )))
        .bind(id)
        .execute(&mut *blocker)
        .await
        .unwrap();
        let task_store = store.clone();
        let task_fixture = fixture.clone();
        let pending = tokio::spawn(async move {
            create_order(
                &SalesService::new(task_store, "SO".into(), "SHP".into(), 30),
                &task_fixture,
                NaiveDate::from_ymd_opt(2026, 9, 20).unwrap(),
                table,
            )
            .await
        });
        blocked_pid(&pool, pid).await;
        blocker.commit().await.unwrap();
        let result = pending.await.unwrap();
        assert!(
            matches!(
                result,
                Err(business_core::b2::DomainError::NotFoundOrForbidden)
            ),
            "disabled {table} must reject order: {result:?}"
        );
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM sales_orders WHERE customer_id=$1")
                .bind(customer)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0);
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET status='active' WHERE id=$1"
        )))
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    }
    // Hold the order insert after validation has acquired its customer share lock.
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,'business_master_data:manage' FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING")
        .bind(fixture.actor).execute(&pool).await.unwrap();
    let mut insert_gate = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE sales_orders IN SHARE MODE")
        .execute(&mut *insert_gate)
        .await
        .unwrap();
    let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *insert_gate)
        .await
        .unwrap();
    let task_store = store.clone();
    let task_fixture = fixture.clone();
    let order = tokio::spawn(async move {
        let service = SalesService::new(task_store, "SO".into(), "SHP".into(), 30);
        create_order(
            &service,
            &task_fixture,
            NaiveDate::from_ymd_opt(2026, 9, 20).unwrap(),
            "master-active-control",
        )
        .await
    });
    let order_pid = blocked_pid(&pool, gate_pid).await;
    let actor = fixture.actor;
    let expected_version: i64 =
        sqlx::query_scalar("SELECT version FROM business_customers WHERE id=$1")
            .bind(customer)
            .fetch_one(&pool)
            .await
            .unwrap();
    let disable = tokio::spawn(async move {
        use business_core::master_data::{
            ChangeCoreMasterStatus, CoreMasterDataService, CoreMasterType,
        };
        CoreMasterDataService::new(store)
            .change_status(
                actor,
                Uuid::new_v4(),
                CoreMasterType::Customer,
                customer,
                "disable-after-order",
                &ChangeCoreMasterStatus {
                    status: "disabled".into(),
                    expected_version,
                },
            )
            .await
    });
    blocked_pid(&pool, order_pid).await;
    insert_gate.commit().await.unwrap();
    assert!(
        order.await.unwrap().is_ok(),
        "active customer order must complete"
    );
    let disabled = disable.await.unwrap();
    assert!(
        matches!(&disabled, Err(business_core::b2::DomainError::Invalid(message)) if message.contains("blocking operational impacts")),
        "unexpected disable result: {disabled:?}"
    );
    let status: String = sqlx::query_scalar("SELECT status FROM business_customers WHERE id=$1")
        .bind(customer)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "active");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sales_orders WHERE customer_id=$1")
        .bind(customer)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
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
async fn create_order(
    sales: &SalesService,
    fixture: &Fixture,
    date: NaiveDate,
    key: &str,
) -> Result<business_core::b2::model::CommandResult, business_core::b2::DomainError> {
    sales
        .create_order(
            fixture.actor,
            Uuid::new_v4(),
            key,
            &CreateSalesOrder {
                legal_entity_id: fixture.legal_entity,
                customer_id: fixture.customer,
                salesperson_user_id: None,
                business_unit_id: fixture.business_unit,
                department_id: None,
                brand_id: Some(fixture.brand),
                currency: "CNY".into(),
                order_date: date,
                requested_delivery_date: Some(date),
                payment_terms_days: None,
                customer_reference: None,
                business_note: None,
                lines: vec![SalesOrderLineInput {
                    sku_id: fixture.sku,
                    warehouse_id: fixture.warehouse,
                    unit_of_measure_id: fixture.uom,
                    quantity: dec(8),
                    unit_price: dec(100),
                    discount_amount: dec(0),
                    tax_rate: dec(0),
                    business_unit_id: None,
                    department_id: None,
                    brand_id: Some(fixture.brand),
                }],
            },
        )
        .await
}
