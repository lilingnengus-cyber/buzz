use super::*;
use business_core::master_data::{ChangeCoreMasterStatus, CoreMasterDataService, CoreMasterType};

pub(super) async fn check(pool: &sqlx::PgPool, store: &PgStore, f: &Fixture) {
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT r.role_id,p.key FROM business_user_roles r CROSS JOIN (VALUES ('business_master_data:manage'),('business_master_data:read')) p(key) WHERE r.enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).execute(pool).await.unwrap();
    let original_version: i64 =
        sqlx::query_scalar("SELECT version FROM business_warehouses WHERE id=$1")
            .bind(f.warehouse)
            .fetch_one(pool)
            .await
            .unwrap();
    let original_command = business_core::master_data::CoreMasterCommand::ChangeStatus {
        resource_type: "warehouse".into(),
        document_id: f.warehouse,
        command: ChangeCoreMasterStatus {
            status: "disabled".into(),
            expected_version: original_version,
        },
    };
    let original_preview = CoreMasterDataService::new(store.clone())
        .command_preview(f.actor, &original_command)
        .await
        .unwrap();
    let mut gate = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE purchase_orders IN SHARE MODE")
        .execute(&mut *gate)
        .await
        .unwrap();
    let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    let service = PurchasingService::new(store.clone(), "PO".into(), 30);
    let task = service.clone();
    let fixture = f.clone();
    let creation = tokio::spawn(async move {
        create_order(
            &task,
            &fixture,
            NaiveDate::from_ymd_opt(2026, 9, 20).unwrap(),
            "warehouse-reverse-draft",
            "1",
            "10",
        )
        .await
    });
    let order_pid = blocked_pid(pool, gate_pid).await;
    let expected_version: i64 =
        sqlx::query_scalar("SELECT version FROM business_warehouses WHERE id=$1")
            .bind(f.warehouse)
            .fetch_one(pool)
            .await
            .unwrap();
    let master = CoreMasterDataService::new(store.clone());
    let actor = f.actor;
    let warehouse = f.warehouse;
    let disable = tokio::spawn(async move {
        master
            .change_status(
                actor,
                Uuid::new_v4(),
                CoreMasterType::Warehouse,
                warehouse,
                "warehouse-reverse-disable",
                &ChangeCoreMasterStatus {
                    status: "disabled".into(),
                    expected_version,
                },
            )
            .await
    });
    blocked_pid(pool, order_pid).await;
    gate.commit().await.unwrap();
    let order = creation.await.unwrap();
    let result = disable.await.unwrap();
    assert!(
        matches!(&result,Err(DomainError::Invalid(message)) if message.contains("blocking operational impacts")),
        "warehouse disable must observe committed draft: {result:?}"
    );
    let master = CoreMasterDataService::new(store.clone());
    let impact = master
        .impact(f.actor, CoreMasterType::Warehouse, f.warehouse)
        .await
        .unwrap();
    assert!(!impact.can_disable);
    assert!(matches!(
        master
            .save_guarded(
                f.actor,
                Uuid::new_v4(),
                "status-stale-new-order",
                &original_command,
                &original_preview
            )
            .await,
        Err(DomainError::StalePreview)
    ));

    assert!(impact
        .impacts
        .iter()
        .any(|v| v.code == "purchase_inbound" && v.blocking && v.count == 1));
    let status: String = sqlx::query_scalar("SELECT status FROM business_warehouses WHERE id=$1")
        .bind(f.warehouse)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(status, "active");
    service
        .cancel_remaining(
            f.actor,
            Uuid::new_v4(),
            order.id,
            "warehouse-reverse-cleanup",
            &version(1),
        )
        .await
        .unwrap();
    assert!(
        master
            .impact(f.actor, CoreMasterType::Warehouse, f.warehouse)
            .await
            .unwrap()
            .can_disable
    );
    let disabled = master
        .change_status(
            f.actor,
            Uuid::new_v4(),
            CoreMasterType::Warehouse,
            f.warehouse,
            "warehouse-disable-after-cancel",
            &ChangeCoreMasterStatus {
                status: "disabled".into(),
                expected_version,
            },
        )
        .await
        .unwrap();
    assert_eq!(disabled.status, "disabled");
    let enabled = master
        .change_status(
            f.actor,
            Uuid::new_v4(),
            CoreMasterType::Warehouse,
            f.warehouse,
            "warehouse-restore-after-cancel",
            &ChangeCoreMasterStatus {
                status: "active".into(),
                expected_version: disabled.version,
            },
        )
        .await
        .unwrap();
    assert_eq!(enabled.status, "active");
    let command = business_core::master_data::CoreMasterCommand::ChangeStatus {
        resource_type: "warehouse".into(),
        document_id: f.warehouse,
        command: ChangeCoreMasterStatus {
            status: "disabled".into(),
            expected_version: enabled.version,
        },
    };
    let preview = master.command_preview(f.actor, &command).await.unwrap();
    let mut tampered = preview.clone();
    tampered["canExecute"] = false.into();
    assert!(matches!(
        master
            .save_guarded(
                f.actor,
                Uuid::new_v4(),
                "status-tampered",
                &command,
                &tampered
            )
            .await,
        Err(DomainError::StalePreview)
    ));
    let disabled = master
        .save_guarded(
            f.actor,
            Uuid::new_v4(),
            "status-guarded",
            &command,
            &preview,
        )
        .await
        .unwrap();
    assert_eq!(disabled.status, "disabled");
    let replay = master
        .save_guarded(
            f.actor,
            Uuid::new_v4(),
            "status-guarded",
            &command,
            &preview,
        )
        .await
        .unwrap();
    assert!(replay.idempotent_replay);
    assert_eq!(replay.version, disabled.version);
    assert!(matches!(
        master
            .save_guarded(
                f.actor,
                Uuid::new_v4(),
                "status-guarded",
                &command,
                &tampered
            )
            .await,
        Err(DomainError::IdempotencyConflict)
    ));
    master
        .change_status(
            f.actor,
            Uuid::new_v4(),
            CoreMasterType::Warehouse,
            f.warehouse,
            "status-guarded-restore",
            &ChangeCoreMasterStatus {
                status: "active".into(),
                expected_version: disabled.version,
            },
        )
        .await
        .unwrap();
}

pub(super) async fn blocked_pid(pool: &sqlx::PgPool, blocker: i32) -> i32 {
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
    .expect("expected actual lock wait")
}
