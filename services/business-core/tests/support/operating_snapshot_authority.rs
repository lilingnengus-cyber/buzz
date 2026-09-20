use business_core::s1::{GenerateOperatingSnapshot, OperationsService};
use chrono::NaiveDate;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn verify(pool: &PgPool, service: &OperationsService, actor: Uuid, day: NaiveDate) {
    let input = GenerateOperatingSnapshot {
        cadence: "daily".into(),
        currency: "CNY".into(),
        period_start: day,
        utc_offset_minutes: 480,
    };
    let key = "snapshot-authority-replay";
    assert!(service
        .generate_operating_snapshot(actor, Uuid::new_v4(), key, &input)
        .await
        .is_ok());
    let roles: Vec<Uuid> = sqlx::query_scalar("DELETE FROM business_role_permissions WHERE permission_key='management_report:generate_snapshot' RETURNING role_id")
        .fetch_all(pool).await.unwrap();
    assert!(!roles.is_empty());
    assert!(service
        .generate_operating_snapshot(actor, Uuid::new_v4(), key, &input)
        .await
        .is_err());
    for role in roles {
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,'management_report:generate_snapshot')").bind(role).execute(pool).await.unwrap();
    }
    let key = "snapshot-scope-replay";
    assert!(service
        .generate_operating_snapshot(actor, Uuid::new_v4(), key, &input)
        .await
        .is_ok());
    let scopes: Vec<(Uuid, Uuid)> = sqlx::query_as("DELETE FROM business_legal_entity_scopes WHERE enterprise_user_id=$1 RETURNING legal_entity_id,granted_by")
        .bind(actor).fetch_all(pool).await.unwrap();
    assert!(!scopes.is_empty());
    assert!(service
        .generate_operating_snapshot(actor, Uuid::new_v4(), key, &input)
        .await
        .is_err());
    for (legal_entity, granted_by) in scopes {
        sqlx::query("INSERT INTO business_legal_entity_scopes(enterprise_user_id,legal_entity_id,granted_by) VALUES($1,$2,$3)")
            .bind(actor).bind(legal_entity).bind(granted_by).execute(pool).await.unwrap();
    }
    sqlx::raw_sql("CREATE FUNCTION reject_snapshot_finish() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.operation='operating_snapshot_generate' AND NEW.idempotency_key='snapshot-finish-failure' THEN RAISE EXCEPTION 'injected snapshot finish failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_snapshot_finish BEFORE UPDATE ON business_command_idempotency FOR EACH ROW EXECUTE FUNCTION reject_snapshot_finish();")
        .execute(pool).await.unwrap();
    let trace = Uuid::new_v4();
    let fresh = GenerateOperatingSnapshot {
        period_start: day.succ_opt().unwrap(),
        ..input
    };
    let error = service
        .generate_operating_snapshot(actor, trace, "snapshot-finish-failure", &fresh)
        .await
        .unwrap_err();
    assert!(
        matches!(error, business_core::b2::DomainError::Database(sqlx::Error::Database(ref error)) if error.message().contains("injected snapshot finish failure"))
    );
    let counts: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM operating_report_snapshots WHERE trace_id=$1),(SELECT count(*) FROM business_core_audit_events WHERE trace_id=$1),(SELECT count(*) FROM business_command_idempotency WHERE idempotency_key='snapshot-finish-failure')")
        .bind(trace).fetch_one(pool).await.unwrap();
    assert_eq!(
        counts,
        (0, 0, 0),
        "failed completion must roll back snapshot, audit and idempotency"
    );
    sqlx::raw_sql("DROP TRIGGER reject_snapshot_finish ON business_command_idempotency; DROP FUNCTION reject_snapshot_finish();").execute(pool).await.unwrap();
    assert_eq!(
        service
            .generate_operating_snapshot(actor, Uuid::new_v4(), "snapshot-finish-failure", &fresh)
            .await
            .unwrap()["created"],
        true
    );
}
