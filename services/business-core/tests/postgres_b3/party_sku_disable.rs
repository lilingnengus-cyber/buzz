use super::*;
use business_core::{
    master_data::{ChangeCoreMasterStatus, CoreMasterDataService, CoreMasterType},
    product_master::{ChangeProductMasterStatus, ProductMasterService, ProductMasterType},
};

pub(super) async fn check(pool: &sqlx::PgPool, store: &PgStore, f: &Fixture) {
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT r.role_id,p.key FROM business_user_roles r CROSS JOIN (VALUES ('business_product_master:manage'),('business_product_master:read')) p(key) WHERE r.enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).execute(pool).await.unwrap();
    let service = PurchasingService::new(store.clone(), "PO".into(), 30);
    for (table, id) in [("business_suppliers", f.supplier), ("business_skus", f.sku)] {
        let sku = table == "business_skus";
        let mut gate = pool.begin().await.unwrap();
        sqlx::query("LOCK TABLE purchase_orders IN SHARE MODE")
            .execute(&mut *gate)
            .await
            .unwrap();
        let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *gate)
            .await
            .unwrap();
        let task = service.clone();
        let fixture = f.clone();
        let creation = tokio::spawn(async move {
            create_order(
                &task,
                &fixture,
                NaiveDate::from_ymd_opt(2026, 9, 20).unwrap(),
                &format!("{table}-reverse-draft"),
                "1",
                "10",
            )
            .await
        });
        let order_pid = warehouse_disable::blocked_pid(pool, gate_pid).await;
        let expected_version: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT version FROM {table} WHERE id=$1"
        )))
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
        let task_store = store.clone();
        let actor = f.actor;
        let disable = tokio::spawn(async move {
            change(
                &task_store,
                actor,
                sku,
                id,
                expected_version,
                "disabled",
                &format!("{table}-reverse-disable"),
            )
            .await
        });
        warehouse_disable::blocked_pid(pool, order_pid).await;
        gate.commit().await.unwrap();
        let order = creation.await.unwrap();
        let result = disable.await.unwrap();
        assert!(
            matches!(&result,Err(DomainError::Invalid(message)) if message.contains("blocking operational impacts")),
            "{table}: {result:?}"
        );
        let status: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT status FROM {table} WHERE id=$1"
        )))
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(status, "active");
        if sku {
            let impact = ProductMasterService::new(store.clone())
                .impact(f.actor, ProductMasterType::Sku, id)
                .await
                .unwrap();
            assert!(!impact.can_disable);
            assert!(impact
                .impacts
                .iter()
                .any(|i| i.code == "purchase_inbound" && i.blocking && i.count == 1));
        } else {
            let impact = CoreMasterDataService::new(store.clone())
                .impact(f.actor, CoreMasterType::Supplier, id)
                .await
                .unwrap();
            assert!(!impact.can_disable);
            assert!(impact
                .impacts
                .iter()
                .any(|i| i.code == "open_orders" && i.blocking && i.count == 1));
        }
        service
            .cancel_remaining(
                f.actor,
                Uuid::new_v4(),
                order.id,
                &format!("{table}-reverse-cleanup"),
                &version(1),
            )
            .await
            .unwrap();
        let version = change(
            store,
            f.actor,
            sku,
            id,
            expected_version,
            "disabled",
            &format!("{table}-disable-after-cancel"),
        )
        .await
        .unwrap();
        change(
            store,
            f.actor,
            sku,
            id,
            version,
            "active",
            &format!("{table}-restore-after-cancel"),
        )
        .await
        .unwrap();
    }
}
async fn change(
    store: &PgStore,
    actor: Uuid,
    sku: bool,
    id: Uuid,
    version: i64,
    status: &str,
    key: &str,
) -> Result<i64, DomainError> {
    if sku {
        ProductMasterService::new(store.clone())
            .change_status(
                actor,
                Uuid::new_v4(),
                ProductMasterType::Sku,
                id,
                key,
                &ChangeProductMasterStatus {
                    status: status.into(),
                    expected_version: version,
                },
            )
            .await
            .map(|r| r.version)
    } else {
        CoreMasterDataService::new(store.clone())
            .change_status(
                actor,
                Uuid::new_v4(),
                CoreMasterType::Supplier,
                id,
                key,
                &ChangeCoreMasterStatus {
                    status: status.into(),
                    expected_version: version,
                },
            )
            .await
            .map(|r| r.version)
    }
}
