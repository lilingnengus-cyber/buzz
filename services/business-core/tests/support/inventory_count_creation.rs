use super::{version, Fixture};
use business_core::{
    b2::{CreateInventoryCount, DomainError, InventoryCountService},
    PgStore,
};
use serde_json::Value;
use uuid::Uuid;

async fn records(store: &PgStore) -> (i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM inventory_count_tasks),(SELECT count(*) FROM inventory_count_events),(SELECT count(*) FROM inventory_movements)").fetch_one(store.pool()).await.unwrap()
}
async fn wait_for_inventory_lock(store: &PgStore) {
    for _ in 0..100 {
        let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND cardinality(pg_blocking_pids(pid))>0 AND query LIKE '%FROM inventory_balances%FOR UPDATE%')").fetch_one(store.pool()).await.unwrap();
        if blocked {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("count creation did not enter the expected inventory lock wait");
}

pub(super) async fn check(
    store: &PgStore,
    service: &InventoryCountService,
    f: &Fixture,
    input: &CreateInventoryCount,
) {
    let before = records(store).await;
    let preview = service.creation_preview(f.actor, input).await.unwrap();
    assert_eq!(preview["command"]["warehouseId"], f.warehouse.to_string());
    assert_eq!(preview["lines"].as_array().unwrap().len(), 1);
    assert_eq!(
        preview["lines"][0]["onHandQuantity"]
            .as_str()
            .unwrap()
            .parse::<rust_decimal::Decimal>()
            .unwrap(),
        rust_decimal::Decimal::ZERO
    );
    assert_eq!(
        service.creation_preview(f.actor, input).await.unwrap(),
        preview
    );
    assert_eq!(records(store).await, before);
    for (key, revoke) in [
        ("count-guarded-stale", "version"),
        ("count-guarded-revoked", "brand"),
        ("count-guarded-disabled-role", "role"),
        ("count-guarded-disabled-user", "user"),
    ] {
        let expected = service.creation_preview(f.actor, input).await.unwrap();
        let mut blocker = store.pool().begin().await.unwrap();
        sqlx::query("SELECT sku_id FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3 FOR UPDATE").bind(f.legal_entity).bind(f.warehouse).bind(f.sku).execute(&mut *blocker).await.unwrap();
        let svc = service.clone();
        let actor = f.actor;
        let command = input.clone();
        let running = tokio::spawn(async move {
            svc.create_guarded(actor, Uuid::new_v4(), key, &command, &expected)
                .await
        });
        wait_for_inventory_lock(store).await;
        if revoke == "brand" {
            sqlx::query(
                "DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2",
            )
            .bind(f.actor)
            .bind(f.brand)
            .execute(store.pool())
            .await
            .unwrap();
        } else if revoke == "role" {
            sqlx::query("UPDATE business_roles SET status='disabled' WHERE id IN (SELECT role_id FROM business_user_roles WHERE enterprise_user_id=$1)").bind(f.actor).execute(store.pool()).await.unwrap();
        } else if revoke == "user" {
            sqlx::query("UPDATE enterprise_users SET status='disabled' WHERE id=$1")
                .bind(f.actor)
                .execute(store.pool())
                .await
                .unwrap();
        } else {
            sqlx::query("UPDATE inventory_balances SET version=version+1 WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3").bind(f.legal_entity).bind(f.warehouse).bind(f.sku).execute(&mut *blocker).await.unwrap();
        }
        blocker.commit().await.unwrap();
        let result = running.await.unwrap();
        if revoke != "version" {
            assert!(matches!(result, Err(DomainError::NotFoundOrForbidden)));
            if revoke == "brand" {
                sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.brand).execute(store.pool()).await.unwrap();
            } else if revoke == "role" {
                sqlx::query("UPDATE business_roles SET status='active' WHERE id IN (SELECT role_id FROM business_user_roles WHERE enterprise_user_id=$1)").bind(f.actor).execute(store.pool()).await.unwrap();
            } else {
                sqlx::query("UPDATE enterprise_users SET status='active' WHERE id=$1")
                    .bind(f.actor)
                    .execute(store.pool())
                    .await
                    .unwrap();
            }
        } else {
            assert!(matches!(result, Err(DomainError::StalePreview)));
        }
        assert_eq!(records(store).await, before);
    }
    let expected = service.creation_preview(f.actor, input).await.unwrap();
    let mut changed = input.clone();
    changed.business_note = Some("changed after preview".into());
    assert!(matches!(
        service
            .create_guarded(
                f.actor,
                Uuid::new_v4(),
                "count-guarded-changed",
                &changed,
                &expected
            )
            .await,
        Err(DomainError::StalePreview)
    ));
    // A one-connection pool detects accidental extra pool acquisition while
    // the creation/replay transaction already owns the only connection.
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect(&std::env::var("BUSINESS_CORE_B2_TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let service = InventoryCountService::new(PgStore::new(pool), "CNT".into());
    let created = service
        .create_guarded(
            f.actor,
            Uuid::new_v4(),
            "count-guarded-success",
            input,
            &expected,
        )
        .await
        .unwrap();
    assert_eq!(created.status, "counting");
    assert!(
        service
            .create_guarded(
                f.actor,
                Uuid::new_v4(),
                "count-guarded-success",
                input,
                &expected
            )
            .await
            .unwrap()
            .idempotent_replay
    );
    let mut tampered: Value = expected.clone();
    tampered["lines"][0]["version"] = 0.into();
    assert!(matches!(
        service
            .create_guarded(
                f.actor,
                Uuid::new_v4(),
                "count-guarded-success",
                input,
                &tampered
            )
            .await,
        Err(DomainError::IdempotencyConflict)
    ));
    assert!(service.creation_preview(f.actor, input).await.is_err());
    service
        .cancel(
            f.actor,
            Uuid::new_v4(),
            created.id,
            "count-guarded-cleanup",
            &version(1),
        )
        .await
        .unwrap();
    assert!(service.creation_preview(f.actor, input).await.is_ok());
}
