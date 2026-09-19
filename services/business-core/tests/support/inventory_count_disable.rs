use super::{version, Fixture};
use business_core::{
    b2::{CreateInventoryCount, DomainError, InventoryCountService},
    product_master::{ChangeProductMasterStatus, ProductMasterService, ProductMasterType},
    PgStore,
};
use uuid::Uuid;

pub(super) async fn check(
    store: &PgStore,
    service: &InventoryCountService,
    f: &Fixture,
    input: &CreateInventoryCount,
) {
    let mut gate = store.pool().begin().await.unwrap();
    sqlx::query("LOCK TABLE inventory_count_tasks IN SHARE MODE")
        .execute(&mut *gate)
        .await
        .unwrap();
    let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    let task = service.clone();
    let command = input.clone();
    let actor = f.actor;
    let creation = tokio::spawn(async move {
        task.create(actor, Uuid::new_v4(), "count-before-sku-disable", &command)
            .await
    });
    let count_pid = blocked_pid(store, gate_pid).await;
    let expected_version: i64 = sqlx::query_scalar("SELECT version FROM business_skus WHERE id=$1")
        .bind(f.sku)
        .fetch_one(store.pool())
        .await
        .unwrap();
    let master = ProductMasterService::new(store.clone());
    let sku = f.sku;
    let disable = tokio::spawn(async move {
        master
            .change_status(
                actor,
                Uuid::new_v4(),
                ProductMasterType::Sku,
                sku,
                "disable-after-count-lock",
                &ChangeProductMasterStatus {
                    status: "disabled".into(),
                    expected_version,
                },
            )
            .await
    });
    blocked_pid(store, count_pid).await;
    gate.commit().await.unwrap();
    let created = creation.await.unwrap().unwrap();
    let result = disable.await.unwrap();
    assert!(
        matches!(result,Err(DomainError::Invalid(ref message)) if message.contains("blocking operational impacts")),
        "{result:?}"
    );
    let status: String = sqlx::query_scalar("SELECT status FROM business_skus WHERE id=$1")
        .bind(sku)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(status, "active");
    assert_eq!(
        service.detail(actor, created.id).await.unwrap().status,
        "counting"
    );
    service
        .cancel(
            actor,
            Uuid::new_v4(),
            created.id,
            "count-before-disable-cleanup",
            &version(1),
        )
        .await
        .unwrap();
    let master = ProductMasterService::new(store.clone());
    let disabled = master
        .change_status(
            actor,
            Uuid::new_v4(),
            ProductMasterType::Sku,
            sku,
            "disable-after-count-cancel",
            &ChangeProductMasterStatus {
                status: "disabled".into(),
                expected_version,
            },
        )
        .await
        .unwrap();
    assert_eq!(disabled.status, "disabled");
    let enabled = master
        .change_status(
            actor,
            Uuid::new_v4(),
            ProductMasterType::Sku,
            sku,
            "restore-after-count-cancel",
            &ChangeProductMasterStatus {
                status: "active".into(),
                expected_version: disabled.version,
            },
        )
        .await
        .unwrap();
    assert_eq!(enabled.status, "active");
}
async fn blocked_pid(store: &PgStore, blocker: i32) -> i32 {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let pid: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(blocker)
            .fetch_optional(store.pool())
            .await
            .unwrap();
            if let Some(pid) = pid {
                return pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("expected real count/master lock wait")
}
