use business_core::b4::{model::GenerateReportSnapshot, ProfitReportingService};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub(super) const INSERT_FACT: &str = "INSERT INTO profit_facts SELECT (jsonb_populate_record(NULL::profit_facts,to_jsonb(f)||jsonb_build_object('id',$1::uuid,'fact_sequence',nextval('business_profit_fact_sequence'),'source_event_id',$2::uuid,'source_line_id',$3::uuid,'amount',$4::text,'management_period','2026-04','business_date','2026-04-01'))).* FROM profit_facts f WHERE metric_type='net_revenue' AND direction='normal' ORDER BY fact_sequence LIMIT 1 RETURNING fact_sequence";

pub async fn verify(pool: &PgPool, service: &ProfitReportingService, actor: Uuid) {
    // Reserve and insert the lower sequence, but deliberately commit it last.
    let mut late = pool.begin().await.unwrap();
    let low: i64 = sqlx::query_scalar(INSERT_FACT)
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind("7.000000")
        .fetch_one(&mut *late)
        .await
        .unwrap();
    let high: i64 = sqlx::query_scalar(INSERT_FACT)
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind("11.000000")
        .fetch_one(pool)
        .await
        .unwrap();
    assert!(low < high);
    let input = GenerateReportSnapshot {
        report_type: "management_profit_statement".into(),
        management_period: "2026-04".into(),
        currency: "CNY".into(),
        legal_entity_ids: vec![],
        supersedes_snapshot_id: None,
    };
    let first = service
        .generate_snapshot(actor, Uuid::new_v4(), "monthly-late-first", &input)
        .await
        .unwrap();
    late.commit().await.unwrap();
    let replacement = GenerateReportSnapshot {
        supersedes_snapshot_id: Some(first.id),
        ..input.clone()
    };
    let second = service
        .generate_snapshot(actor, Uuid::new_v4(), "monthly-late-second", &replacement)
        .await
        .unwrap();
    assert_ne!(
        first.id, second.id,
        "newly committed content must not reuse the old watermark snapshot"
    );
    let read = |id| {
        sqlx::query("SELECT s.source_watermark,s.source_hash,s.supersedes_snapshot_id,r.amounts->'components'->0->>'amount' amount FROM management_report_snapshots s JOIN management_report_snapshot_rows r ON r.snapshot_id=s.id WHERE s.id=$1").bind(id)
    };
    let a = read(first.id).fetch_one(pool).await.unwrap();
    let b = read(second.id).fetch_one(pool).await.unwrap();
    assert_eq!(a.get::<i64, _>("source_watermark"), high);
    assert_eq!(b.get::<i64, _>("source_watermark"), high);
    assert_ne!(
        a.get::<String, _>("source_hash"),
        b.get::<String, _>("source_hash")
    );
    assert_eq!(a.get::<String, _>("amount"), "11.000000");
    assert_eq!(b.get::<String, _>("amount"), "18.000000");
    assert_eq!(
        b.get::<Option<Uuid>, _>("supersedes_snapshot_id"),
        Some(first.id)
    );
    let repeated = service
        .generate_snapshot(actor, Uuid::new_v4(), "monthly-late-repeat", &replacement)
        .await
        .unwrap();
    assert_eq!(repeated.id, second.id);
    assert!(repeated.idempotent_replay);
    // An explicit retry of the old request must retain its original immutable result.
    let old = service
        .generate_snapshot(actor, Uuid::new_v4(), "monthly-late-first", &input)
        .await
        .unwrap();
    assert_eq!(old.id, first.id);
}
