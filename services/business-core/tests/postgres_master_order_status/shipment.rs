use super::*;
use business_core::b2::{
    model::{CreateShipment, ShipmentLineInput, VersionCommand},
    DomainError, InventoryService,
};
use sqlx::Row;

pub(super) async fn check(pool: &sqlx::PgPool, f: &Fixture, product: Uuid, category: Uuid) {
    let sales = SalesService::new(PgStore::new(pool.clone()), "SO".into(), "SHP".into(), 30);
    let inventory = InventoryService::new(PgStore::new(pool.clone()), "OP".into(), "AR".into());
    let order: Uuid = sqlx::query_scalar("SELECT id FROM sales_orders WHERE customer_id=$1")
        .bind(f.customer)
        .fetch_one(pool)
        .await
        .unwrap();
    let line: Uuid = sqlx::query_scalar("SELECT id FROM sales_order_lines WHERE sales_order_id=$1")
        .bind(order)
        .fetch_one(pool)
        .await
        .unwrap();
    let input = CreateShipment {
        sales_order_id: order,
        warehouse_id: f.warehouse,
        shipment_date: NaiveDate::from_ymd_opt(2026, 9, 20).unwrap(),
        lines: vec![ShipmentLineInput {
            sales_order_line_id: line,
            quantity: dec(4),
        }],
    };
    let mut shipment = None;
    for confirming in [false, true] {
        if confirming {
            shipment = Some(
                sales
                    .create_shipment(f.actor, Uuid::new_v4(), "shipment-positive", &input)
                    .await
                    .unwrap()
                    .id,
            );
        }
        for (table, id) in [
            ("business_customers", f.customer),
            ("business_units", f.business_unit),
            ("business_warehouses", f.warehouse),
            ("business_skus", f.sku),
            ("business_products", product),
            ("business_legal_entities", f.legal_entity),
            ("business_units_of_measure", f.uom),
            ("business_product_categories", category),
            ("business_brands", f.brand),
        ] {
            let mut tx = pool.begin().await.unwrap();
            let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {table} SET status='disabled' WHERE id=$1"
            )))
            .bind(id)
            .execute(&mut *tx)
            .await
            .unwrap();
            let task = sales.clone();
            let stock = inventory.clone();
            let actor = f.actor;
            let input = input.clone();
            let pending = tokio::spawn(async move {
                let key = format!("shipment-{confirming}-{table}");
                if let Some(id) = shipment {
                    stock
                        .confirm_shipment(
                            actor,
                            Uuid::new_v4(),
                            id,
                            &key,
                            &VersionCommand {
                                expected_version: 1,
                                reason_code: None,
                            },
                        )
                        .await
                } else {
                    task.create_shipment(actor, Uuid::new_v4(), &key, &input)
                        .await
                }
            });
            blocked_pid(pool, pid).await;
            tx.commit().await.unwrap();
            assert!(
                matches!(
                    pending.await.unwrap(),
                    Err(DomainError::NotFoundOrForbidden)
                ),
                "{confirming}/{table}"
            );
            if let Some(id) = shipment {
                let preview = sales
                    .shipment_confirmation_preview(f.actor, id)
                    .await
                    .unwrap();
                assert!(!preview.can_confirm, "{table}");
                assert_eq!(preview.readiness, "master_data_not_ready");
                assert!(!preview.lines[0].ready);
                assert_eq!(preview.status, "draft");
                assert_eq!(preview.version, 1);
            }
            for table in ["inventory_movements", "trade_receivables"] {
                let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                    "SELECT count(*) FROM {table}"
                )))
                .fetch_one(pool)
                .await
                .unwrap();
                assert_eq!(count, 0, "{table}");
            }
            let balance = sqlx::query(
                "SELECT on_hand_quantity,reserved_quantity FROM inventory_balances WHERE sku_id=$1",
            )
            .bind(f.sku)
            .fetch_one(pool)
            .await
            .unwrap();
            assert_eq!(
                balance.get::<rust_decimal::Decimal, _>("on_hand_quantity"),
                dec(10).0
            );
            assert_eq!(
                balance.get::<rust_decimal::Decimal, _>("reserved_quantity"),
                dec(8).0
            );
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {table} SET status='active' WHERE id=$1"
            )))
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        }
    }
    assert!(
        sales
            .shipment_confirmation_preview(f.actor, shipment.unwrap())
            .await
            .unwrap()
            .can_confirm
    );
    inventory
        .confirm_shipment(
            f.actor,
            Uuid::new_v4(),
            shipment.unwrap(),
            "shipment-confirm-positive",
            &VersionCommand {
                expected_version: 1,
                reason_code: None,
            },
        )
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM trade_receivables")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
