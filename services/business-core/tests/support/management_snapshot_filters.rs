use super::Fixture;
use business_core::{
    b2::DomainError,
    b4::{
        model::{GenerateReportSnapshot, ReportSnapshotFilters},
        ProfitReportingService,
    },
};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn verify(pool: &PgPool, service: &ProfitReportingService, f: &Fixture) {
    for (brand, warehouse, amount) in [
        (Some(f.brand), Some(f.warehouse), "3"),
        (Some(f.brand), None, "5"),
        (None, Some(f.warehouse), "7"),
        (None, None, "11"),
    ] {
        sqlx::query("INSERT INTO profit_facts SELECT (jsonb_populate_record(NULL::profit_facts,to_jsonb(f)||jsonb_build_object('id',$1::uuid,'fact_sequence',nextval('business_profit_fact_sequence'),'source_event_id',$2::uuid,'source_line_id',$3::uuid,'amount',$4::text,'brand_id',$5::uuid,'warehouse_id',$6::uuid,'management_period','2026-01','business_date','2026-01-01'))).* FROM profit_facts f WHERE metric_type='net_revenue' AND direction='normal' ORDER BY fact_sequence LIMIT 1")
            .bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(amount).bind(brand).bind(warehouse).execute(pool).await.unwrap();
    }
    let base = GenerateReportSnapshot {
        report_type: "management_profit_statement".into(),
        management_period: "2026-01".into(),
        currency: "CNY".into(),
        legal_entity_ids: vec![f.legal_entity],
        supersedes_snapshot_id: None,
        filters: None,
    };
    // Old clients keep both their request hash and five-key scope shape.
    assert!(serde_json::to_value(&base)
        .unwrap()
        .get("filters")
        .is_none());
    let legacy = service.snapshot_preview(f.actor, &base).await.unwrap();
    assert_eq!(legacy["scope"].as_object().unwrap().len(), 5);
    assert_eq!(legacy["components"][0]["amount"], "26.000000");
    let original = service
        .generate_snapshot_guarded(f.actor, Uuid::new_v4(), "filter-legacy", &base, &legacy)
        .await
        .unwrap();
    for (brand, warehouse, amount) in [
        (true, false, "8.000000"),
        (false, true, "10.000000"),
        (true, true, "3.000000"),
    ] {
        let input = GenerateReportSnapshot {
            filters: Some(ReportSnapshotFilters {
                brand_ids: brand.then(|| vec![f.brand]),
                warehouse_ids: warehouse.then(|| vec![f.warehouse]),
                customer_ids: Some(vec![f.customer]),
                business_unit_ids: Some(vec![f.business_unit]),
            }),
            ..base.clone()
        };
        let preview = service.snapshot_preview(f.actor, &input).await.unwrap();
        assert_eq!(preview["components"][0]["amount"], amount);
        if brand {
            assert_eq!(preview["scope"]["includeUnassignedBrand"], false);
        }
        if warehouse {
            assert_eq!(preview["scope"]["includeUnassignedWarehouse"], false);
        }
        assert_ne!(preview["scopeHash"], legacy["scopeHash"]);
        let result = service
            .generate_snapshot_guarded(
                f.actor,
                Uuid::new_v4(),
                &format!("filter-{brand}-{warehouse}"),
                &input,
                &preview,
            )
            .await
            .unwrap();
        assert_ne!(result.id, original.id);
        let wrong_parent = GenerateReportSnapshot {
            supersedes_snapshot_id: Some(original.id),
            ..input
        };
        assert!(matches!(
            service.snapshot_preview(f.actor, &wrong_parent).await,
            Err(DomainError::Invalid(_))
        ));
    }
    for ids in [vec![], vec![Uuid::new_v4()]] {
        let input = GenerateReportSnapshot {
            filters: Some(ReportSnapshotFilters {
                brand_ids: Some(ids),
                ..Default::default()
            }),
            ..base.clone()
        };
        assert!(service.snapshot_preview(f.actor, &input).await.is_err());
    }
    let replay = service
        .generate_snapshot_guarded(f.actor, Uuid::new_v4(), "filter-legacy", &base, &legacy)
        .await
        .unwrap();
    assert_eq!(replay.id, original.id);
    let stored: serde_json::Value = sqlx::query_scalar(
        "SELECT amounts FROM management_report_snapshot_rows WHERE snapshot_id=$1",
    )
    .bind(original.id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(stored["components"][0]["amount"], json!("26.000000"));
}
