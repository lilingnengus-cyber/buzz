use business_core::s1::{GenerateOperatingSnapshot, OperationsService};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn verify(pool: &PgPool, service: &OperationsService, actor: Uuid, day: NaiveDate) {
    let before: Decimal =
        sqlx::query_scalar("SELECT sum(inventory_value)::numeric(24,6) FROM inventory_balances")
            .fetch_one(pool)
            .await
            .unwrap();
    let quality = service.data_quality(actor).await.unwrap()["status"].clone();
    let mut blocker = pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("LOCK TABLE purchase_orders IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let service = service.clone();
    let run = tokio::spawn(async move {
        service
            .generate_operating_snapshot(
                actor,
                Uuid::new_v4(),
                "snapshot-consistent-read",
                &GenerateOperatingSnapshot {
                    cadence: "daily".into(),
                    currency: "CNY".into(),
                    period_start: day.pred_opt().unwrap(),
                    legal_entity_ids: None,
                    utc_offset_minutes: 480,
                },
            )
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
    assert!(
        waiting,
        "snapshot must actually wait after establishing its read snapshot"
    );
    // A concurrent committed balance change also changes reconciliation quality.
    sqlx::query("UPDATE inventory_balances SET inventory_value=inventory_value+1")
        .execute(pool)
        .await
        .unwrap();
    blocker.commit().await.unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let id = Uuid::parse_str(result["id"].as_str().unwrap()).unwrap();
    let (value, status): (Decimal, String) = sqlx::query_as("SELECT (payload->>'inventoryValueAsOfGeneration')::numeric,data_quality_status FROM operating_report_snapshots WHERE id=$1")
        .bind(id).fetch_one(pool).await.unwrap();
    assert_eq!(
        value, before,
        "snapshot metrics must not mix in a later committed balance"
    );
    assert_eq!(
        serde_json::Value::String(status),
        quality,
        "quality must share the metrics transaction snapshot"
    );
    sqlx::query("UPDATE inventory_balances SET inventory_value=inventory_value-1")
        .execute(pool)
        .await
        .unwrap();
}
