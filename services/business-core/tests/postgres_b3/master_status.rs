use super::*;

pub(super) async fn check(pool: &sqlx::PgPool, store: &PgStore, fixture: &Fixture) {
    let product: Uuid = sqlx::query_scalar("SELECT product_id FROM business_skus WHERE id=$1")
        .bind(fixture.sku)
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
    sqlx::query("INSERT INTO business_brands(id,code,name) VALUES($1,'MASTER_WAIT_BRAND','Master wait brand')").bind(brand).execute(pool).await.unwrap();
    sqlx::query("UPDATE business_products SET brand_id=$1 WHERE id=$2")
        .bind(brand)
        .bind(product)
        .execute(pool)
        .await
        .unwrap();
    for (table, id) in [
        ("business_suppliers", fixture.supplier),
        ("business_units", fixture.business_unit),
        ("business_warehouses", fixture.warehouse),
        ("business_skus", fixture.sku),
        ("business_products", product),
        ("business_legal_entities", fixture.legal_entity),
        ("business_units_of_measure", fixture.uom),
        ("business_product_categories", category),
        ("business_brands", brand),
    ] {
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
        let f = fixture.clone();
        let service = PurchasingService::new(store.clone(), "PO".into(), 30);
        let request = tokio::spawn(async move {
            create_order(
                &service,
                &f,
                NaiveDate::from_ymd_opt(2026, 9, 20).unwrap(),
                table,
                "1",
                "10",
            )
            .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(10),async {
            loop {
                let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))").bind(pid).fetch_one(pool).await.unwrap();
                if waiting {break;}
                assert!(!request.is_finished(),"{table}: request did not wait");
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        tx.commit().await.unwrap();
        let result = request.await.unwrap();
        assert!(
            matches!(
                result,
                Err(DomainError::NotFoundOrForbidden) | Err(DomainError::Invalid(_))
            ),
            "{table}: {result:?}"
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM purchase_orders")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET status='active' WHERE id=$1"
        )))
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }
    let service = PurchasingService::new(store.clone(), "PO".into(), 30);
    let draft = create_order(
        &service,
        fixture,
        NaiveDate::from_ymd_opt(2026, 9, 20).unwrap(),
        "master-confirm-draft",
        "1",
        "10",
    )
    .await
    .unwrap();
    for (table, id) in [
        ("business_suppliers", fixture.supplier),
        ("business_units", fixture.business_unit),
        ("business_warehouses", fixture.warehouse),
        ("business_skus", fixture.sku),
        ("business_products", product),
        ("business_legal_entities", fixture.legal_entity),
        ("business_units_of_measure", fixture.uom),
        ("business_product_categories", category),
        ("business_brands", brand),
    ] {
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
        let confirmation_service = service.clone();
        let actor = fixture.actor;
        let order_id = draft.id;
        let request = tokio::spawn(async move {
            confirmation_service
                .confirm_order(
                    actor,
                    Uuid::new_v4(),
                    order_id,
                    &format!("confirm-{table}"),
                    &version(1),
                )
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(10),async {
            loop {
                let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))").bind(pid).fetch_one(pool).await.unwrap();
                if waiting {break;}
                assert!(!request.is_finished(),"confirmation did not wait for {table}");
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        tx.commit().await.unwrap();
        assert!(matches!(
            request.await.unwrap(),
            Err(DomainError::Invalid(_)) | Err(DomainError::NotFoundOrForbidden)
        ));
        assert!(
            !service
                .confirmation_preview(fixture.actor, draft.id)
                .await
                .unwrap()
                .can_confirm,
            "preview must block {table}"
        );
        let state: (String, i64) =
            sqlx::query_as("SELECT lifecycle_status,version FROM purchase_orders WHERE id=$1")
                .bind(draft.id)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(state, ("draft".into(), 1));
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET status='active' WHERE id=$1"
        )))
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }
    service
        .confirm_order(
            fixture.actor,
            Uuid::new_v4(),
            draft.id,
            "master-confirm-positive",
            &version(1),
        )
        .await
        .unwrap();
    service
        .cancel_remaining(
            fixture.actor,
            Uuid::new_v4(),
            draft.id,
            "master-confirm-cleanup",
            &VersionCommand {
                expected_version: 2,
                reason_code: Some("test completed".into()),
            },
        )
        .await
        .unwrap();
    sqlx::query("UPDATE business_products SET brand_id=NULL WHERE id=$1")
        .bind(product)
        .execute(pool)
        .await
        .unwrap();
}
async fn create_order(
    service: &PurchasingService,
    f: &Fixture,
    date: NaiveDate,
    key: &str,
    quantity: &str,
    unit_price: &str,
) -> Result<business_core::b3::model::CommandResult, DomainError> {
    service
        .create_order(
            f.actor,
            Uuid::new_v4(),
            key,
            &CreatePurchaseOrder {
                legal_entity_id: f.legal_entity,
                supplier_id: f.supplier,
                buyer_user_id: Some(f.actor),
                business_unit_id: f.business_unit,
                department_id: None,
                brand_id: None,
                currency: "CNY".into(),
                order_date: date,
                expected_delivery_date: Some(date),
                payment_terms_days: Some(30),
                supplier_reference: None,
                business_note: None,
                lines: vec![PurchaseOrderLineInput {
                    sku_id: f.sku,
                    warehouse_id: f.warehouse,
                    unit_of_measure_id: f.uom,
                    quantity: dec(quantity),
                    unit_price: dec(unit_price),
                    discount_amount: dec("0"),
                    tax_rate: dec("0"),
                    business_unit_id: None,
                    department_id: None,
                    brand_id: None,
                }],
            },
        )
        .await
}
