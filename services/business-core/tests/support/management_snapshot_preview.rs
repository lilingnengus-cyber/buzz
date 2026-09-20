use business_core::{
    b2::DomainError,
    b4::{model::GenerateReportSnapshot, ProfitReportingService},
};
use sqlx::PgPool;
use uuid::Uuid;

async fn counts(pool: &PgPool) -> (i64, i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM management_report_snapshots),(SELECT count(*) FROM business_core_audit_events),(SELECT count(*) FROM business_command_idempotency),(SELECT COALESCE(sum(current_value),0)::bigint FROM business_numbering_sequence_pools)").fetch_one(pool).await.unwrap()
}
pub async fn verify(pool: &PgPool, service: &ProfitReportingService, actor: Uuid) {
    let input = GenerateReportSnapshot {
        report_type: "management_profit_statement".into(),
        management_period: "2026-05".into(),
        currency: "CNY".into(),
        legal_entity_ids: vec![],
        supersedes_snapshot_id: None,
        filters: None,
    };
    let before = counts(pool).await;
    let preview = service.snapshot_preview(actor, &input).await.unwrap();
    assert_eq!(
        before,
        counts(pool).await,
        "preview must not persist records or allocate a number"
    );
    assert_eq!(preview["effects"]["createsImmutableSnapshot"], true);
    let mut tampered = preview.clone();
    tampered["sourceHash"] = serde_json::json!("tampered");
    assert!(matches!(
        service
            .generate_snapshot_guarded(
                actor,
                Uuid::new_v4(),
                "monthly-preview-tamper",
                &input,
                &tampered
            )
            .await,
        Err(DomainError::StalePreview)
    ));
    assert_eq!(before, counts(pool).await);
    let insert = super::management_snapshot_late_fact::INSERT_FACT.replace("2026-04", "2026-05");
    sqlx::query(sqlx::AssertSqlSafe(insert))
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind("12.000000")
        .execute(pool)
        .await
        .unwrap();
    assert!(matches!(
        service
            .generate_snapshot_guarded(
                actor,
                Uuid::new_v4(),
                "monthly-preview-stale",
                &input,
                &preview
            )
            .await,
        Err(DomainError::StalePreview)
    ));
    assert_eq!(
        before,
        counts(pool).await,
        "stale preview leaves no idempotency or snapshot effects"
    );
    let current = service.snapshot_preview(actor, &input).await.unwrap();
    assert_eq!(current["components"][0]["amount"], "12.000000");
    // A later approval failure must roll back report, numbering, audit and idempotency.
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await
        .unwrap();
    let rolled_back = service
        .generate_snapshot_on(
            &mut tx,
            actor,
            Uuid::new_v4(),
            "monthly-preview-outer-rollback",
            &input,
            Some(&current),
        )
        .await
        .unwrap();
    assert!(!rolled_back.idempotent_replay);
    tx.rollback().await.unwrap();
    assert_eq!(before, counts(pool).await);
    let mut weak_tx = pool.begin().await.unwrap();
    assert!(matches!(
        service
            .generate_snapshot_on(
                &mut weak_tx,
                actor,
                Uuid::new_v4(),
                "monthly-preview-weak-isolation",
                &input,
                Some(&current),
            )
            .await,
        Err(DomainError::Invalid(_))
    ));
    weak_tx.rollback().await.unwrap();
    assert_eq!(before, counts(pool).await);
    let result = service
        .generate_snapshot_guarded(
            actor,
            Uuid::new_v4(),
            "monthly-preview-create",
            &input,
            &current,
        )
        .await
        .unwrap();
    assert!(!result.idempotent_replay);
    let replay = service
        .generate_snapshot_guarded(
            actor,
            Uuid::new_v4(),
            "monthly-preview-create",
            &input,
            &current,
        )
        .await
        .unwrap();
    assert_eq!(replay.id, result.id);
    assert!(replay.idempotent_replay);
    assert!(matches!(
        service
            .generate_snapshot_guarded(
                actor,
                Uuid::new_v4(),
                "monthly-preview-create",
                &input,
                &tampered
            )
            .await,
        Err(DomainError::IdempotencyConflict)
    ));
    let existing = service.snapshot_preview(actor, &input).await.unwrap();
    assert_eq!(existing["effects"]["createsImmutableSnapshot"], false);
    assert_eq!(existing["existingSnapshot"]["id"], result.id.to_string());
    assert!(existing["existingSnapshot"]["generatedAt"].is_string());
    assert!(existing["existingSnapshot"]["dataAsOf"].is_string());
    let reused = service
        .generate_snapshot_guarded(
            actor,
            Uuid::new_v4(),
            "monthly-preview-existing",
            &input,
            &existing,
        )
        .await
        .unwrap();
    assert_eq!(reused.id, result.id);
    assert!(reused.idempotent_replay);
    let after = counts(pool).await;
    assert_eq!(after.0, before.0 + 1);
}
