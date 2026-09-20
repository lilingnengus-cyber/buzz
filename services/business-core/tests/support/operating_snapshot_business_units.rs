use super::Fixture;
use business_core::s1::{GenerateOperatingSnapshot, OperationsService};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
fn amount(v: &Value, key: &str) -> Decimal {
    v["metrics"][key].as_str().unwrap().parse().unwrap()
}
pub async fn verify(pool: &PgPool, service: &OperationsService, f: &Fixture) {
    let unit = Uuid::new_v4();
    sqlx::query("INSERT INTO business_units(id,legal_entity_id,code,name) VALUES($1,$2,'REPORT_STOCK_UNIT','Stock ownership unit')").bind(unit).bind(f.legal_entity).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(unit).execute(pool).await.unwrap();
    // Deliberately separate warehouse ownership from the sales order's business unit.
    sqlx::query("UPDATE business_warehouses SET business_unit_id=$2 WHERE id=$1")
        .bind(f.warehouse)
        .bind(unit)
        .execute(pool)
        .await
        .unwrap();
    for cadence in ["daily", "weekly"] {
        let base = GenerateOperatingSnapshot {
            cadence: cadence.into(),
            currency: "CNY".into(),
            period_start: NaiveDate::from_ymd_opt(
                2026,
                8,
                if cadence == "daily" { 21 } else { 17 },
            )
            .unwrap(),
            utc_offset_minutes: 480,
            legal_entity_ids: None,
            business_unit_ids: None,
        };
        assert!(serde_json::to_value(&base)
            .unwrap()
            .get("businessUnitIds")
            .is_none());
        let full = service
            .operating_snapshot_preview(f.actor, &base)
            .await
            .unwrap();
        assert!(amount(&full, "inventoryValueAsOfGeneration") > Decimal::ZERO);
        assert!(amount(&full, "shippedRevenue") > Decimal::ZERO);
        for selected in [f.business_unit, unit] {
            let input = GenerateOperatingSnapshot {
                business_unit_ids: Some(vec![selected, selected]),
                ..base.clone()
            };
            let preview = service
                .operating_snapshot_preview(f.actor, &input)
                .await
                .unwrap();
            assert_eq!(preview["scope"]["businessUnitIds"], json!([selected]));
            assert_eq!(preview["schemaVersion"], 2);
            assert_ne!(preview["scopeHash"], full["scopeHash"]);
            assert_eq!(
                amount(&preview, "inventoryValueAsOfGeneration"),
                if selected == unit {
                    amount(&full, "inventoryValueAsOfGeneration")
                } else {
                    Decimal::ZERO
                }
            );
            for key in [
                "salesOrderAmount",
                "shippedRevenue",
                "managementOperatingProfit",
            ] {
                assert_eq!(
                    amount(&preview, key),
                    if selected == f.business_unit {
                        amount(&full, key)
                    } else {
                        Decimal::ZERO
                    },
                    "{key}"
                );
            }
            for key in [
                "incidentsOpened",
                "incidentsResolved",
                "slaBreached",
                "averageResolutionHours",
            ] {
                assert!(preview["metrics"][key].is_null());
                assert_eq!(
                    preview["metrics"]["unavailableMetrics"][key],
                    "not_attributable_to_selected_business_units"
                );
            }
            assert_ne!(preview["dataQualityStatus"], "complete");
            let key = format!("bu-{cadence}-{selected}");
            let result = service
                .generate_operating_snapshot_guarded(
                    f.actor,
                    Uuid::new_v4(),
                    &key,
                    &input,
                    &preview,
                )
                .await
                .unwrap();
            assert_eq!(
                service
                    .generate_operating_snapshot_guarded(
                        f.actor,
                        Uuid::new_v4(),
                        &key,
                        &input,
                        &preview
                    )
                    .await
                    .unwrap(),
                result
            );
            let detail = service
                .operating_snapshot_detail(f.actor, result["id"].as_str().unwrap().parse().unwrap())
                .await
                .unwrap();
            assert_eq!(detail["metrics"], preview["metrics"]);
            assert_eq!(detail["scope"], preview["scope"]);
        }
        for ids in [vec![], vec![Uuid::new_v4()]] {
            let invalid = GenerateOperatingSnapshot {
                business_unit_ids: Some(ids),
                ..base.clone()
            };
            assert!(service
                .operating_snapshot_preview(f.actor, &invalid)
                .await
                .is_err());
        }
        let all = GenerateOperatingSnapshot {
            business_unit_ids: Some(vec![unit, f.business_unit]),
            ..base
        };
        let preview = service
            .operating_snapshot_preview(f.actor, &all)
            .await
            .unwrap();
        assert_eq!(
            amount(&preview, "inventoryValueAsOfGeneration"),
            amount(&full, "inventoryValueAsOfGeneration")
        );
        assert!(preview["metrics"]["slaBreached"].is_null());
    }
    let prior = GenerateOperatingSnapshot {
        cadence: "daily".into(),
        currency: "CNY".into(),
        period_start: NaiveDate::from_ymd_opt(2026, 9, 14).unwrap(),
        utc_offset_minutes: 480,
        legal_entity_ids: None,
        business_unit_ids: None,
    };
    let previous = service
        .generate_operating_snapshot(f.actor, Uuid::new_v4(), "bu-previous-unfiltered", &prior)
        .await
        .unwrap();
    let selected = GenerateOperatingSnapshot {
        period_start: NaiveDate::from_ymd_opt(2026, 9, 15).unwrap(),
        business_unit_ids: Some(vec![unit, f.business_unit]),
        ..prior
    };
    let next = service
        .generate_operating_snapshot(f.actor, Uuid::new_v4(), "bu-next-all-selected", &selected)
        .await
        .unwrap();
    let scope_of = |v: &Value| v["id"].as_str().unwrap().parse::<Uuid>().unwrap();
    let previous_detail = service
        .operating_snapshot_detail(f.actor, scope_of(&previous))
        .await
        .unwrap();
    let next_detail = service
        .operating_snapshot_detail(f.actor, scope_of(&next))
        .await
        .unwrap();
    assert_eq!(previous_detail["scope"], next_detail["scope"]);
    let series = service
        .operating_trends(f.actor, "daily", "CNY", 60)
        .await
        .unwrap();
    let row = series["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == next["id"])
        .unwrap();
    assert!(
        row["comparisonSnapshotId"].is_null(),
        "same UUID sets do not imply same metric attribution"
    );
    // A granted unit outside the selected legal entity must fail, not silently become empty.
    let other_legal: Uuid =
        sqlx::query_scalar("SELECT id FROM business_legal_entities WHERE code='EMPTY_REPORT_LE'")
            .fetch_one(pool)
            .await
            .unwrap();
    let other_unit = Uuid::new_v4();
    sqlx::query("INSERT INTO business_units(id,legal_entity_id,code,name) VALUES($1,$2,'REPORT_OTHER_LE_UNIT','Other legal unit')").bind(other_unit).bind(other_legal).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(other_unit).execute(pool).await.unwrap();
    let incompatible = GenerateOperatingSnapshot {
        legal_entity_ids: Some(vec![f.legal_entity]),
        business_unit_ids: Some(vec![other_unit]),
        ..selected
    };
    assert!(service
        .operating_snapshot_preview(f.actor, &incompatible)
        .await
        .is_err());
    sqlx::query(
        "DELETE FROM business_unit_scopes WHERE enterprise_user_id=$1 AND business_unit_id=$2",
    )
    .bind(f.actor)
    .bind(other_unit)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE business_warehouses SET business_unit_id=$2 WHERE id=$1")
        .bind(f.warehouse)
        .bind(f.business_unit)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM business_unit_scopes WHERE enterprise_user_id=$1 AND business_unit_id=$2",
    )
    .bind(f.actor)
    .bind(unit)
    .execute(pool)
    .await
    .unwrap();
}
