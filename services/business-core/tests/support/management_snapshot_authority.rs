use business_core::{
    b2::DomainError,
    b4::{model::GenerateReportSnapshot, ProfitReportingService},
};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn verify(pool: &PgPool, service: &ProfitReportingService, actor: Uuid) {
    let input = GenerateReportSnapshot {
        report_type: "management_profit_statement".into(),
        management_period: "2026-08".into(),
        currency: "CNY".into(),
        legal_entity_ids: vec![],
        supersedes_snapshot_id: None,
        filters: None,
    };
    let key = "monthly-snapshot-authority";
    let original = service
        .generate_snapshot(actor, Uuid::new_v4(), key, &input)
        .await
        .unwrap();
    let scopes:Vec<(Uuid,Uuid)>=sqlx::query_as("DELETE FROM business_legal_entity_scopes WHERE enterprise_user_id=$1 RETURNING legal_entity_id,granted_by")
        .bind(actor).fetch_all(pool).await.unwrap();
    assert!(!scopes.is_empty());
    assert!(matches!(
        service
            .generate_snapshot(actor, Uuid::new_v4(), key, &input)
            .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    let replacement = GenerateReportSnapshot {
        supersedes_snapshot_id: Some(original.id),
        ..input.clone()
    };
    assert!(matches!(
        service
            .generate_snapshot(
                actor,
                Uuid::new_v4(),
                "monthly-outside-predecessor",
                &replacement
            )
            .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    for (legal_entity, granted_by) in scopes {
        sqlx::query("INSERT INTO business_legal_entity_scopes(enterprise_user_id,legal_entity_id,granted_by) VALUES($1,$2,$3)")
            .bind(actor).bind(legal_entity).bind(granted_by).execute(pool).await.unwrap();
    }
    let wrong_period = GenerateReportSnapshot {
        management_period: "2026-07".into(),
        ..replacement
    };
    let trace = Uuid::new_v4();
    assert!(matches!(
        service
            .generate_snapshot(actor, trace, "monthly-wrong-predecessor", &wrong_period)
            .await,
        Err(DomainError::Invalid(_))
    ));
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM management_report_snapshots WHERE trace_id=$1),(SELECT count(*) FROM business_core_audit_events WHERE trace_id=$1),(SELECT count(*) FROM business_command_idempotency WHERE idempotency_key='monthly-wrong-predecessor')")
        .bind(trace).fetch_one(pool).await.unwrap();
    assert_eq!(counts, (0, 0, 0));
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT revision FROM business_authorization_revision WHERE singleton FOR UPDATE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    let service = service.clone();
    let trace = Uuid::new_v4();
    let run = tokio::spawn(async move {
        service
            .generate_snapshot(actor, trace, "monthly-wait-revoked", &input)
            .await
    });
    let mut waiting = false;
    for _ in 0..200 {
        waiting = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
        )
        .bind(pid)
        .fetch_one(pool)
        .await
        .unwrap();
        if waiting {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(waiting);
    let roles:Vec<Uuid>=sqlx::query_scalar("DELETE FROM business_role_permissions WHERE permission_key='management_report:generate_snapshot' RETURNING role_id").fetch_all(&mut *blocker).await.unwrap();
    assert!(!roles.is_empty());
    blocker.commit().await.unwrap();
    assert!(matches!(
        tokio::time::timeout(std::time::Duration::from_secs(10), run)
            .await
            .unwrap()
            .unwrap(),
        Err(DomainError::NotFoundOrForbidden)
    ));
    let remaining:i64=sqlx::query_scalar("SELECT count(*) FROM business_command_idempotency WHERE idempotency_key='monthly-wait-revoked'").fetch_one(pool).await.unwrap();
    assert_eq!(remaining, 0);
    for role in roles {
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,'management_report:generate_snapshot')").bind(role).execute(pool).await.unwrap();
    }
}
