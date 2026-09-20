//! Source-attribution regressions against the isolated reporting B4 fixture.
use super::*;
fn count(v: &Value, domain: &str) -> i64 {
    v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["domain"] == domain)
        .unwrap()["differenceCount"]
        .as_i64()
        .unwrap()
}
#[tokio::test]
async fn warehouse_quality_separates_inventory_receivables_and_payables() {
    let Ok(url) = std::env::var("BUSINESS_OPERATING_WAREHOUSE_QUALITY_TEST_DATABASE_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    let service = OperationsService::new(crate::PgStore::new(pool.clone()), true, 60);
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await
        .unwrap();
    let first: Uuid = sqlx::query_scalar("SELECT id FROM business_warehouses WHERE code='WH_B4'")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let second: Uuid =
        sqlx::query_scalar("SELECT id FROM business_warehouses WHERE code='REPORT_SECOND_WH'")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let actor: Uuid = sqlx::query_scalar(
        "SELECT enterprise_user_id FROM business_warehouse_scopes WHERE warehouse_id=$1 LIMIT 1",
    )
    .bind(first)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let before_first = service
        .data_quality_for_snapshot_on(&mut tx, actor, None, None, Some(&[first]))
        .await
        .unwrap();
    let before_second = service
        .data_quality_for_snapshot_on(&mut tx, actor, None, None, Some(&[second]))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE inventory_balances SET inventory_value=inventory_value+1 WHERE warehouse_id=$1",
    )
    .bind(first)
    .execute(&mut *tx)
    .await
    .unwrap();
    let receivable:Uuid=sqlx::query_scalar("SELECT t.id FROM trade_receivables t JOIN shipments s ON s.id=t.shipment_id WHERE s.warehouse_id=$1 AND t.open_amount>=1 AND t.status<>'reversed' LIMIT 1").bind(first).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("UPDATE trade_receivables SET settled_amount=settled_amount+1,open_amount=open_amount-1 WHERE id=$1").bind(receivable).execute(&mut *tx).await.unwrap();
    let payable:Uuid=sqlx::query_scalar("SELECT p.id FROM trade_payables p JOIN goods_receipts g ON g.id=p.goods_receipt_id WHERE g.warehouse_id=$1 AND p.open_amount>=1 AND p.status<>'reversed' LIMIT 1").bind(second).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("UPDATE trade_payables SET settled_amount=settled_amount+1,open_amount=open_amount-1 WHERE id=$1").bind(payable).execute(&mut *tx).await.unwrap();
    let after_first = service
        .data_quality_for_snapshot_on(&mut tx, actor, None, None, Some(&[first]))
        .await
        .unwrap();
    let after_second = service
        .data_quality_for_snapshot_on(&mut tx, actor, None, None, Some(&[second]))
        .await
        .unwrap();
    for domain in ["inventory", "receivables"] {
        assert!(
            count(&after_first, domain) > count(&before_first, domain),
            "{domain}"
        );
        assert_eq!(
            count(&after_second, domain),
            count(&before_second, domain),
            "{domain}"
        );
    }
    assert_eq!(
        count(&after_first, "payables"),
        count(&before_first, "payables")
    );
    assert_eq!(
        count(&after_second, "payables"),
        count(&before_second, "payables") + 1
    );
    tx.rollback().await.unwrap();
}
#[tokio::test]
async fn payable_quality_follows_purchase_business_unit() {
    let Ok(url) = std::env::var("BUSINESS_OPERATING_WAREHOUSE_QUALITY_TEST_DATABASE_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    let service = OperationsService::new(crate::PgStore::new(pool.clone()), true, 60);
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await
        .unwrap();
    let other: Uuid =
        sqlx::query_scalar("SELECT id FROM business_units WHERE code='REPORT_STOCK_UNIT'")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let (payable,order,unit,actor):(Uuid,Uuid,Uuid,Uuid)=sqlx::query_as("SELECT p.id,o.id,o.business_unit_id,o.created_by_user_id FROM trade_payables p JOIN purchase_orders o ON o.id=p.purchase_order_id WHERE p.open_amount>=1 AND p.status<>'reversed' LIMIT 1").fetch_one(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1) ON CONFLICT DO NOTHING").bind(actor).bind(other).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE purchase_orders SET business_unit_id=$2 WHERE id=$1")
        .bind(order)
        .bind(other)
        .execute(&mut *tx)
        .await
        .unwrap();
    let before = service
        .data_quality_for_snapshot_on(&mut tx, actor, None, Some(&[other]), None)
        .await
        .unwrap();
    let outside = service
        .data_quality_for_snapshot_on(&mut tx, actor, None, Some(&[unit]), None)
        .await
        .unwrap();
    sqlx::query("UPDATE trade_payables SET settled_amount=settled_amount+1,open_amount=open_amount-1 WHERE id=$1").bind(payable).execute(&mut *tx).await.unwrap();
    let after = service
        .data_quality_for_snapshot_on(&mut tx, actor, None, Some(&[other]), None)
        .await
        .unwrap();
    let untouched = service
        .data_quality_for_snapshot_on(&mut tx, actor, None, Some(&[unit]), None)
        .await
        .unwrap();
    assert_eq!(count(&after, "payables"), count(&before, "payables") + 1);
    assert_eq!(count(&untouched, "payables"), count(&outside, "payables"));
    tx.rollback().await.unwrap();
}
