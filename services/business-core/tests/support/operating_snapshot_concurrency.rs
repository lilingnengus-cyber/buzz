use business_core::{
    b2::DomainError,
    s1::{GenerateOperatingSnapshot, OperationsService},
};
use chrono::{Duration, NaiveDate};
use sqlx::PgPool;
use uuid::Uuid;

async fn wait_for_blockers(pool: &PgPool, count: i64) {
    for _ in 0..300 {
        let waiting: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND cardinality(pg_blocking_pids(pid))>0")
            .fetch_one(pool).await.unwrap();
        if waiting >= count {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("expected {count} actual database waiters");
}
fn input(day: NaiveDate) -> GenerateOperatingSnapshot {
    GenerateOperatingSnapshot {
        cadence: "daily".into(),
        currency: "CNY".into(),
        period_start: day,
        business_unit_ids: None,
        warehouse_ids: None,
        legal_entity_ids: None,
        utc_offset_minutes: 480,
    }
}

pub async fn verify(pool: &PgPool, service: &OperationsService, actor: Uuid, day: NaiveDate) {
    sqlx::raw_sql("CREATE FUNCTION pause_snapshot_finish() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.operation='operating_snapshot_generate' AND NEW.idempotency_key LIKE 'snapshot-race-%' THEN PERFORM pg_advisory_xact_lock(202609201); END IF; RETURN NEW; END $$; CREATE TRIGGER pause_snapshot_finish BEFORE UPDATE ON business_command_idempotency FOR EACH ROW EXECUTE FUNCTION pause_snapshot_finish();")
        .execute(pool).await.unwrap();
    for (offset, same_key) in [(2, true), (3, false)] {
        let mut blocker = pool.begin().await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock(202609201)")
            .execute(&mut *blocker)
            .await
            .unwrap();
        let first_service = service.clone();
        let request = input(day - Duration::days(offset));
        let first_input = request.clone();
        let first_key = format!("snapshot-race-{offset}-first");
        let second_key = if same_key {
            first_key.clone()
        } else {
            format!("snapshot-race-{offset}-second")
        };
        let first = tokio::spawn(async move {
            first_service
                .generate_operating_snapshot(actor, Uuid::new_v4(), &first_key, &first_input)
                .await
        });
        wait_for_blockers(pool, 1).await;
        let second_service = service.clone();
        let second = tokio::spawn(async move {
            second_service
                .generate_operating_snapshot(actor, Uuid::new_v4(), &second_key, &request)
                .await
        });
        wait_for_blockers(pool, 2).await;
        blocker.commit().await.unwrap();
        let a = tokio::time::timeout(std::time::Duration::from_secs(10), first)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let b = tokio::time::timeout(std::time::Duration::from_secs(10), second)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            a["id"], b["id"],
            "competing requests must resolve to the same frozen snapshot"
        );
        if !same_key {
            assert_eq!(b["created"], false);
        }
        let audits:i64=sqlx::query_scalar("SELECT count(*) FROM business_core_audit_events WHERE operation='operating_snapshot.generate' AND target_id=$1")
            .bind(a["id"].as_str().unwrap()).fetch_one(pool).await.unwrap();
        assert_eq!(audits, 1);
    }
    sqlx::raw_sql("DROP TRIGGER pause_snapshot_finish ON business_command_idempotency; DROP FUNCTION pause_snapshot_finish();").execute(pool).await.unwrap();
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT revision FROM business_authorization_revision WHERE singleton FOR UPDATE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let service = service.clone();
    let trace = Uuid::new_v4();
    let run = tokio::spawn(async move {
        service
            .generate_operating_snapshot(
                actor,
                trace,
                "snapshot-race-revoked",
                &input(day - Duration::days(4)),
            )
            .await
    });
    wait_for_blockers(pool, 1).await;
    let roles:Vec<Uuid>=sqlx::query_scalar("DELETE FROM business_role_permissions WHERE permission_key='management_report:generate_snapshot' RETURNING role_id")
        .fetch_all(&mut *blocker).await.unwrap();
    assert!(!roles.is_empty());
    blocker.commit().await.unwrap();
    let error = tokio::time::timeout(std::time::Duration::from_secs(10), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(
        matches!(error, DomainError::NotFoundOrForbidden),
        "retry must recheck revoked authority: {error:?}"
    );
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM operating_report_snapshots WHERE trace_id=$1),(SELECT count(*) FROM business_core_audit_events WHERE trace_id=$1),(SELECT count(*) FROM business_command_idempotency WHERE idempotency_key='snapshot-race-revoked')")
        .bind(trace).fetch_one(pool).await.unwrap();
    assert_eq!(counts, (0, 0, 0));
    for role in roles {
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,'management_report:generate_snapshot')").bind(role).execute(pool).await.unwrap();
    }
}
