//! Optional regression against the isolated B4 fixture; all mutations roll back.
use super::*;
#[tokio::test]
async fn business_unit_quality_uses_source_order_and_warehouse_ownership() {
    let Ok(url) = std::env::var("BUSINESS_OPERATING_QUALITY_TEST_DATABASE_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    let service = OperationsService::new(crate::PgStore::new(pool.clone()), true, 60);
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await
        .unwrap();
    let stock_unit: Uuid =
        sqlx::query_scalar("SELECT id FROM business_units WHERE code='REPORT_STOCK_UNIT'")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let (actor, warehouse, sales_unit): (Uuid,Uuid,Uuid)=sqlx::query_as("SELECT sc.enterprise_user_id,w.id,w.business_unit_id FROM business_warehouse_scopes sc JOIN business_warehouses w ON w.id=sc.warehouse_id ORDER BY w.id LIMIT 1").fetch_one(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1) ON CONFLICT DO NOTHING").bind(actor).bind(stock_unit).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE business_warehouses SET business_unit_id=$2 WHERE id=$1")
        .bind(warehouse)
        .bind(stock_unit)
        .execute(&mut *tx)
        .await
        .unwrap();
    let count = |v: &Value, domain: &str| -> i64 {
        v["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["domain"] == domain)
            .unwrap()["differenceCount"]
            .as_i64()
            .unwrap()
    };
    let stock_before = service
        .data_quality_for_snapshot_on(&mut tx, actor, None, Some(&[stock_unit]), None)
        .await
        .unwrap();
    let sales_before = service
        .data_quality_for_snapshot_on(&mut tx, actor, None, Some(&[sales_unit]), None)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE inventory_balances SET inventory_value=inventory_value+1 WHERE warehouse_id=$1",
    )
    .bind(warehouse)
    .execute(&mut *tx)
    .await
    .unwrap();
    let receivable: Uuid=sqlx::query_scalar("SELECT t.id FROM trade_receivables t JOIN sales_orders o ON o.id=t.sales_order_id WHERE o.business_unit_id=$1 AND t.open_amount>=1 AND t.status<>'reversed' LIMIT 1").bind(sales_unit).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("UPDATE trade_receivables SET settled_amount=settled_amount+1,open_amount=open_amount-1 WHERE id=$1").bind(receivable).execute(&mut *tx).await.unwrap();
    let stock_after = service
        .data_quality_for_snapshot_on(&mut tx, actor, None, Some(&[stock_unit]), None)
        .await
        .unwrap();
    let sales_after = service
        .data_quality_for_snapshot_on(&mut tx, actor, None, Some(&[sales_unit]), None)
        .await
        .unwrap();
    assert!(count(&stock_after, "inventory") > count(&stock_before, "inventory"));
    assert_eq!(
        count(&sales_after, "inventory"),
        count(&sales_before, "inventory")
    );
    assert!(count(&sales_after, "receivables") > count(&sales_before, "receivables"));
    assert_eq!(
        count(&stock_after, "receivables"),
        count(&stock_before, "receivables")
    );
    tx.rollback().await.unwrap();
}
