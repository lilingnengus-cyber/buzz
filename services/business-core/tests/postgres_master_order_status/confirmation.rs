use super::*;
use business_core::b2::{model::VersionCommand, DomainError};

pub(super) async fn check(pool: &sqlx::PgPool, f: &Fixture, product: Uuid, category: Uuid) {
    let sales = SalesService::new(PgStore::new(pool.clone()), "SO".into(), "SHP".into(), 30);
    let order: Uuid = sqlx::query_scalar("SELECT id FROM sales_orders WHERE customer_id=$1")
        .bind(f.customer)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO inventory_balances(legal_entity_id,warehouse_id,sku_id,on_hand_quantity,inventory_value,average_unit_cost) VALUES($1,$2,$3,10,100,10)")
        .bind(f.legal_entity).bind(f.warehouse).bind(f.sku).execute(pool).await.unwrap();
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
        let actor = f.actor;
        let pending = tokio::spawn(async move {
            task.confirm_order(
                actor,
                Uuid::new_v4(),
                order,
                table,
                &VersionCommand {
                    expected_version: 1,
                    reason_code: None,
                },
            )
            .await
        });
        blocked_pid(pool, pid).await;
        tx.commit().await.unwrap();
        assert!(
            matches!(
                pending.await.unwrap(),
                Err(DomainError::NotFoundOrForbidden)
            ),
            "{table}"
        );
        let preview = sales.confirmation_preview(f.actor, order).await.unwrap();
        assert!(!preview.can_confirm, "{table}");
        assert_eq!(preview.readiness, "master_data_not_ready");
        assert_eq!(preview.lifecycle_status, "draft");
        assert_eq!(preview.version, 1);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM inventory_reservations")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        let reserved: rust_decimal::Decimal =
            sqlx::query_scalar("SELECT reserved_quantity FROM inventory_balances WHERE sku_id=$1")
                .bind(f.sku)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(reserved, rust_decimal::Decimal::ZERO);
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET status='active' WHERE id=$1"
        )))
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }
    assert!(
        sales
            .confirmation_preview(f.actor, order)
            .await
            .unwrap()
            .can_confirm
    );
    sales
        .confirm_order(
            f.actor,
            Uuid::new_v4(),
            order,
            "confirm-active",
            &VersionCommand {
                expected_version: 1,
                reason_code: None,
            },
        )
        .await
        .unwrap();
}
