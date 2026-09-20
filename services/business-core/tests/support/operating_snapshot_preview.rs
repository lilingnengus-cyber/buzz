use business_core::{
    b2::DomainError,
    s1::{GenerateOperatingSnapshot, OperationsService},
};
use chrono::NaiveDate;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

async fn counts(pool: &PgPool) -> (i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM operating_report_snapshots),(SELECT count(*) FROM business_core_audit_events),(SELECT count(*) FROM business_command_idempotency)").fetch_one(pool).await.unwrap()
}
pub async fn verify(pool: &PgPool, service: &OperationsService, actor: Uuid) {
    for cadence in ["daily", "weekly"] {
        let input = GenerateOperatingSnapshot {
            cadence: cadence.into(),
            currency: "CNY".into(),
            period_start: NaiveDate::from_ymd_opt(2026, 1, 12).unwrap(),
            utc_offset_minutes: 480,
        };
        let before = counts(pool).await;
        let preview = service
            .operating_snapshot_preview(actor, &input)
            .await
            .unwrap();
        assert_eq!(preview["effects"]["createsImmutableSnapshot"], true);
        assert_eq!(preview["ownerUserId"], json!(actor));
        assert_eq!(before, counts(pool).await);
        let mut tampered = preview.clone();
        tampered["sourceHash"] = json!("wrong");
        assert!(matches!(
            service
                .generate_operating_snapshot_guarded(
                    actor,
                    Uuid::new_v4(),
                    &format!("operating-{cadence}-tampered"),
                    &input,
                    &tampered
                )
                .await,
            Err(DomainError::StalePreview)
        ));
        assert_eq!(before, counts(pool).await);
        sqlx::query("UPDATE inventory_balances SET inventory_value=inventory_value+1")
            .execute(pool)
            .await
            .unwrap();
        assert!(matches!(
            service
                .generate_operating_snapshot_guarded(
                    actor,
                    Uuid::new_v4(),
                    &format!("operating-{cadence}-stale"),
                    &input,
                    &preview
                )
                .await,
            Err(DomainError::StalePreview)
        ));
        assert_eq!(before, counts(pool).await);
        sqlx::query("UPDATE inventory_balances SET inventory_value=inventory_value-1")
            .execute(pool)
            .await
            .unwrap();
        let current = service
            .operating_snapshot_preview(actor, &input)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await
            .unwrap();
        let rolled = service
            .generate_operating_snapshot_guarded_on(
                &mut tx,
                actor,
                Uuid::new_v4(),
                &format!("operating-{cadence}-rollback"),
                &input,
                &current,
            )
            .await
            .unwrap();
        assert_eq!(rolled["created"], true);
        tx.rollback().await.unwrap();
        assert_eq!(before, counts(pool).await);
        let mut weak = pool.begin().await.unwrap();
        assert!(matches!(
            service
                .operating_snapshot_preview_on(&mut weak, actor, &input)
                .await,
            Err(DomainError::Invalid(_))
        ));
        weak.rollback().await.unwrap();
        let key = format!("operating-{cadence}-guarded");
        let created = service
            .generate_operating_snapshot_guarded(actor, Uuid::new_v4(), &key, &input, &current)
            .await
            .unwrap();
        assert_eq!(created["created"], true);
        let id: Uuid = created["id"].as_str().unwrap().parse().unwrap();
        let stored: Value =
            sqlx::query_scalar("SELECT payload FROM operating_report_snapshots WHERE id=$1")
                .bind(id)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(stored, current["metrics"]);
        assert_eq!(created["sourceHash"], current["sourceHash"]);
        let replay = service
            .generate_operating_snapshot_guarded(actor, Uuid::new_v4(), &key, &input, &current)
            .await
            .unwrap();
        assert_eq!(replay, created);
        assert!(matches!(
            service
                .generate_operating_snapshot_guarded(actor, Uuid::new_v4(), &key, &input, &tampered)
                .await,
            Err(DomainError::IdempotencyConflict)
        ));
        let existing = service
            .operating_snapshot_preview(actor, &input)
            .await
            .unwrap();
        assert_eq!(existing["effects"]["createsImmutableSnapshot"], false);
        assert_eq!(existing["existingSnapshot"]["id"], created["id"]);
        sqlx::query("UPDATE inventory_balances SET inventory_value=inventory_value+1")
            .execute(pool)
            .await
            .unwrap();
        assert_eq!(
            service
                .operating_snapshot_preview(actor, &input)
                .await
                .unwrap(),
            existing,
            "existing preview must display frozen metrics, not current metrics"
        );
        let reused = service
            .generate_operating_snapshot_guarded(
                actor,
                Uuid::new_v4(),
                &format!("{key}-reuse"),
                &input,
                &existing,
            )
            .await
            .unwrap();
        assert_eq!(reused["id"], created["id"]);
        assert_eq!(reused["created"], false);
        sqlx::query("UPDATE inventory_balances SET inventory_value=inventory_value-1")
            .execute(pool)
            .await
            .unwrap();
        let after = counts(pool).await;
        assert_eq!(after.0, before.0 + 1);
        assert_eq!(after.1, before.1 + 1);
    }
    let extreme = GenerateOperatingSnapshot {
        cadence: "daily".into(),
        currency: "CNY".into(),
        period_start: NaiveDate::MAX,
        utc_offset_minutes: 480,
    };
    assert!(matches!(
        service.operating_snapshot_preview(actor, &extreme).await,
        Err(DomainError::Invalid(_))
    ));
}
