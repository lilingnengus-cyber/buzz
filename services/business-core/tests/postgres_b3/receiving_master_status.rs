use super::*;
pub(super) async fn check(pool: &sqlx::PgPool, f: &Fixture) {
    let store = PgStore::new(pool.clone());
    let purchase = PurchasingService::new(store.clone(), "PO".into(), 30);
    let service = ReceivingService::new(store, purchase.clone(), "GR".into(), "AP".into());
    let date = NaiveDate::from_ymd_opt(2026, 9, 20).unwrap();
    let order = create_order(&purchase, f, date, "receipt-master-order", "1", "10").await;
    purchase
        .confirm_order(
            f.actor,
            Uuid::new_v4(),
            order.id,
            "receipt-master-confirm",
            &version(1),
        )
        .await
        .unwrap();
    let line: Uuid =
        sqlx::query_scalar("SELECT id FROM purchase_order_lines WHERE purchase_order_id=$1")
            .bind(order.id)
            .fetch_one(pool)
            .await
            .unwrap();
    let product: Uuid = sqlx::query_scalar("SELECT product_id FROM business_skus WHERE id=$1")
        .bind(f.sku)
        .fetch_one(pool)
        .await
        .unwrap();
    let category: Uuid =
        sqlx::query_scalar("SELECT category_id FROM business_products WHERE id=$1")
            .bind(product)
            .fetch_one(pool)
            .await
            .unwrap();
    let brand = Uuid::new_v4();
    sqlx::query("INSERT INTO business_brands(id,code,name) VALUES($1,'RECEIVING_WAIT_BRAND','Receiving wait brand')").bind(brand).execute(pool).await.unwrap();
    sqlx::query("UPDATE business_products SET brand_id=$1 WHERE id=$2")
        .bind(brand)
        .bind(product)
        .execute(pool)
        .await
        .unwrap();
    let input = CreateGoodsReceipt {
        purchase_order_id: order.id,
        warehouse_id: f.warehouse,
        receipt_date: date,
        lines: vec![GoodsReceiptLineInput {
            purchase_order_line_id: line,
            quantity: dec("1"),
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
        ("business_suppliers", f.supplier),
        ("business_brands", brand),
    ];
    let baseline: i64 = sqlx::query_scalar("SELECT count(*) FROM inventory_movements")
        .fetch_one(pool)
        .await
        .unwrap();
    let mut receipt = None;
    for confirming in [false, true] {
        if confirming {
            receipt = Some(
                service
                    .create_receipt(f.actor, Uuid::new_v4(), "receipt-master-positive", &input)
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
            let task = service.clone();
            let actor = f.actor;
            let input = input.clone();
            let request = tokio::spawn(async move {
                let key = format!("receipt-{confirming}-{table}");
                if let Some(id) = receipt {
                    task.confirm_receipt(actor, Uuid::new_v4(), id, &key, &version(1))
                        .await
                } else {
                    task.create_receipt(actor, Uuid::new_v4(), &key, &input)
                        .await
                }
            });
            tokio::time::timeout(std::time::Duration::from_secs(10),async {loop {
                let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))").bind(pid).fetch_one(pool).await.unwrap();
                if waiting {break;}assert!(!request.is_finished(),"{confirming}/{table} must wait");
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }}).await.unwrap();
            tx.commit().await.unwrap();
            let result = request.await.unwrap();
            assert!(
                matches!(result, Err(DomainError::NotFoundOrForbidden)),
                "{confirming}/{table}: {result:?}"
            );
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM inventory_movements")
                .fetch_one(pool)
                .await
                .unwrap();
            assert_eq!(count, baseline);
            if let Some(id) = receipt {
                let preview = service.confirmation_preview(f.actor, id).await.unwrap();
                assert!(!preview.can_confirm, "{table}");
                assert_eq!(preview.readiness, "master_data_not_ready");
                assert!(!preview.lines[0].ready);
                assert_eq!(preview.status, "draft");
                assert_eq!(preview.version, 1);
            }
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
        service
            .confirmation_preview(f.actor, receipt.unwrap())
            .await
            .unwrap()
            .can_confirm
    );
    assert_eq!(
        service
            .confirm_receipt(
                f.actor,
                Uuid::new_v4(),
                receipt.unwrap(),
                "receipt-confirm-positive",
                &version(1)
            )
            .await
            .unwrap()
            .status,
        "confirmed"
    );
}
