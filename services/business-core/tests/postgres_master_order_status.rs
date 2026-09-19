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
async fn sales_create_rechecks_customer_after_disable_wait() {
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
    let mut blocker = pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM business_customers WHERE id=$1 FOR UPDATE")
        .bind(customer)
        .execute(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("UPDATE business_customers SET status='disabled' WHERE id=$1")
        .bind(customer)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let task_store = store.clone();
    let task_fixture = fixture.clone();
    let pending = tokio::spawn(async move {
        let service = SalesService::new(task_store, "SO".into(), "SHP".into(), 30);
        create_order(
            &service,
            &task_fixture,
            NaiveDate::from_ymd_opt(2026, 9, 20).unwrap(),
            "master-disable-wait",
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(pid)
            .fetch_one(&pool)
            .await
            .unwrap();
            if waiting {
                break;
            }
            assert!(!pending.is_finished(), "order must wait for the master row");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    blocker.commit().await.unwrap();
    let result = pending.await.unwrap();
    assert!(
        matches!(
            result,
            Err(business_core::b2::DomainError::NotFoundOrForbidden)
        ),
        "order must reject a customer disabled while validation waited"
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sales_orders WHERE customer_id=$1")
        .bind(customer)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    sqlx::query("UPDATE business_customers SET status='active' WHERE id=$1")
        .bind(customer)
        .execute(&pool)
        .await
        .unwrap();
    let service = SalesService::new(store, "SO".into(), "SHP".into(), 30);
    assert!(create_order(
        &service,
        &fixture,
        NaiveDate::from_ymd_opt(2026, 9, 20).unwrap(),
        "master-active-control"
    )
    .await
    .is_ok());
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
