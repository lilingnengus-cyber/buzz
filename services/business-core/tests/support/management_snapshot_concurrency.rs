use business_core::b4::{model::GenerateReportSnapshot, ProfitReportingService};
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
fn input(month: i64) -> GenerateReportSnapshot {
    GenerateReportSnapshot {
        report_type: "management_profit_statement".into(),
        currency: "CNY".into(),
        management_period: format!("2026-{month:02}"),
        legal_entity_ids: vec![],
        supersedes_snapshot_id: None,
    }
}

pub async fn verify(pool: &PgPool, service: &ProfitReportingService, actor: Uuid) {
    sqlx::raw_sql("CREATE FUNCTION pause_monthly_finish() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.operation='management_report:generate_snapshot' AND NEW.idempotency_key LIKE 'monthly-race-%' THEN PERFORM pg_advisory_xact_lock(202609202); END IF; RETURN NEW; END $$; CREATE TRIGGER pause_monthly_finish BEFORE UPDATE ON business_command_idempotency FOR EACH ROW EXECUTE FUNCTION pause_monthly_finish();")
        .execute(pool).await.unwrap();
    for (offset, same_key) in [(2, true), (3, false)] {
        let mut blocker = pool.begin().await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock(202609202)")
            .execute(&mut *blocker)
            .await
            .unwrap();
        let first_service = service.clone();
        let request = input(offset);
        let first_input = request.clone();
        let first_key = format!("monthly-race-{offset}-first");
        let second_key = if same_key {
            first_key.clone()
        } else {
            format!("monthly-race-{offset}-second")
        };
        let first = tokio::spawn(async move {
            first_service
                .generate_snapshot(actor, Uuid::new_v4(), &first_key, &first_input)
                .await
        });
        wait_for_blockers(pool, 1).await;
        let second_service = service.clone();
        let second = tokio::spawn(async move {
            second_service
                .generate_snapshot(actor, Uuid::new_v4(), &second_key, &request)
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
            a.id, b.id,
            "competing requests must resolve to the same frozen snapshot"
        );
        if !same_key {
            assert!(b.idempotent_replay);
        }
        let audits:i64=sqlx::query_scalar("SELECT count(*) FROM business_core_audit_events WHERE operation='MANAGEMENT_REPORT_SNAPSHOT_GENERATED' AND target_id=$1")
            .bind(a.id.to_string()).fetch_one(pool).await.unwrap();
        assert_eq!(audits, 1);
    }
    sqlx::raw_sql("DROP TRIGGER pause_monthly_finish ON business_command_idempotency; DROP FUNCTION pause_monthly_finish();").execute(pool).await.unwrap();
}
