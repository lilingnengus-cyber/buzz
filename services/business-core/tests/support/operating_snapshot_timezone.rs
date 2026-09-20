use business_core::s1::{GenerateOperatingSnapshot, OperationsService};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

fn input(cadence: &str, offset: i16, day: u32) -> GenerateOperatingSnapshot {
    GenerateOperatingSnapshot {
        cadence: cadence.into(),
        currency: "CNY".into(),
        period_start: NaiveDate::from_ymd_opt(2026, 1, day).unwrap(),
        business_unit_ids: None,
        legal_entity_ids: None,
        utc_offset_minutes: offset,
    }
}
pub async fn verify(pool: &PgPool, service: &OperationsService, actor: Uuid) {
    let preview = service
        .operating_snapshot_preview(actor, &input("daily", 480, 19))
        .await
        .unwrap();
    let scope = preview["scopeHash"].as_str().unwrap();
    for (i, timestamp) in [
        "2026-01-18T15:59:59Z",
        "2026-01-18T16:00:00Z",
        "2026-01-18T20:00:00Z",
        "2026-01-19T10:00:00Z",
        "2026-01-19T16:00:00Z",
        "2026-01-20T00:00:00Z",
    ]
    .iter()
    .enumerate()
    {
        let first = timestamp.parse::<DateTime<Utc>>().unwrap();
        sqlx::query("INSERT INTO operating_report_incidents(id,scope_hash,alert_code,severity,message,evidence_path,condition_status,review_status,first_seen_at,resolved_at,due_at,created_by_user_id,last_trace_id) VALUES($1,$2,$3,'warning','timezone boundary','/api/v1/test','cleared','resolved',$4,$4+interval '1 hour',$4+interval '2 hours',$5,$6)")
            .bind(Uuid::new_v4()).bind(scope).bind(format!("TZ_BOUNDARY_{i}")).bind(first).bind(actor).bind(Uuid::new_v4()).execute(pool).await.unwrap();
    }
    sqlx::query("INSERT INTO operating_report_incidents(id,scope_hash,alert_code,severity,message,evidence_path,condition_status,review_status,first_seen_at,resolved_at,due_at,created_by_user_id,last_trace_id) VALUES($1,$2,'TZ_FUTURE_BREACH','warning','future breach','/api/v1/test','cleared','resolved','2026-01-18T17:00:00Z','2026-01-21T00:00:00Z','2026-01-20T10:00:00Z',$3,$4)")
        .bind(Uuid::new_v4()).bind(scope).bind(actor).bind(Uuid::new_v4()).execute(pool).await.unwrap();
    let mut daily = Vec::new();
    for (cadence, offset, opened, resolved) in [
        ("daily", 480, 4, 4),
        ("daily", 0, 2, 2),
        ("weekly", 480, 6, 7),
        ("weekly", 0, 3, 4),
    ] {
        let request = input(cadence, offset, 19);
        let before = service
            .operating_snapshot_preview(actor, &request)
            .await
            .unwrap();
        assert_eq!(
            before["metrics"]["incidentsOpened"], opened,
            "{cadence} {offset}"
        );
        assert_eq!(
            before["metrics"]["incidentsResolved"], resolved,
            "{cadence} {offset}"
        );
        if cadence == "daily" {
            assert_eq!(before["metrics"]["slaBreached"], 0);
        }
        if offset == 480 {
            assert_eq!(before["periodStartUtc"], "2026-01-18T16:00:00Z");
        }
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SET LOCAL TIME ZONE 'America/Los_Angeles'")
            .execute(&mut *tx)
            .await
            .unwrap();
        let changed_zone = service
            .operating_snapshot_preview_on(&mut tx, actor, &request)
            .await
            .unwrap();
        assert_eq!(
            before, changed_zone,
            "database timezone must not change period metrics"
        );
        tx.rollback().await.unwrap();
        let result = service
            .generate_operating_snapshot_guarded(
                actor,
                Uuid::new_v4(),
                &format!("timezone-{cadence}-{offset}"),
                &request,
                &before,
            )
            .await
            .unwrap();
        assert_eq!(result["utcOffsetMinutes"], offset);
        if cadence == "daily" {
            daily.push((offset, result));
        }
    }
    assert_ne!(daily[0].1["id"], daily[1].1["id"]);
    assert_ne!(daily[0].1["sourceHash"], daily[1].1["sourceHash"]);
    // Old rows remain unknown; a new request cannot mistake them for a zoned snapshot.
    let legacy = Uuid::new_v4();
    sqlx::query("INSERT INTO operating_report_snapshots(id,cadence,period_start,period_end,currency,scope_hash,payload,data_quality_status,source_hash,generated_by_user_id,trace_id) SELECT $1,cadence,period_start,period_end,currency,scope_hash,payload,data_quality_status,source_hash,generated_by_user_id,trace_id FROM operating_report_snapshots WHERE id=$2")
        .bind(legacy).bind(daily[0].1["id"].as_str().unwrap().parse::<Uuid>().unwrap()).execute(pool).await.unwrap();
    for (offset, current) in &daily {
        let previous = service
            .generate_operating_snapshot(
                actor,
                Uuid::new_v4(),
                &format!("timezone-previous-{offset}"),
                &input("daily", *offset, 18),
            )
            .await
            .unwrap();
        let existing = service
            .operating_snapshot_preview(actor, &input("daily", *offset, 19))
            .await
            .unwrap();
        assert_eq!(existing["existingSnapshot"]["id"], current["id"]);
        let trends = service
            .operating_trends(actor, "daily", "CNY", 60)
            .await
            .unwrap();
        let rows = trends["items"].as_array().unwrap();
        let row = rows.iter().find(|r| r["id"] == current["id"]).unwrap();
        assert_eq!(row["comparisonSnapshotId"], previous["id"]);
        assert_eq!(row["utcOffsetMinutes"], *offset);
        let old = rows.iter().find(|r| r["id"] == json!(legacy)).unwrap();
        assert_eq!(old["timeBasis"], "legacy_unknown");
        assert!(old["change"].is_null());
    }
    let extreme = GenerateOperatingSnapshot {
        period_start: NaiveDate::MIN,
        ..input("daily", 840, 19)
    };
    assert!(service
        .operating_snapshot_preview(actor, &extreme)
        .await
        .is_err());
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM operating_report_snapshots WHERE period_start='2026-01-19' AND cadence='daily' AND scope_hash=$1").bind(scope).fetch_one(pool).await.unwrap();
    assert_eq!(count, 3);
    let legacy_offset: Option<i16> =
        sqlx::query_scalar("SELECT utc_offset_minutes FROM operating_report_snapshots WHERE id=$1")
            .bind(legacy)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(legacy_offset, None);
}
