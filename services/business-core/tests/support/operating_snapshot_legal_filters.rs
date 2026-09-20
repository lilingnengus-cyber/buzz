use super::Fixture;
use business_core::s1::{GenerateOperatingSnapshot, OperationsService};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
pub async fn verify(pool: &PgPool, service: &OperationsService, f: &Fixture) {
    let empty = Uuid::new_v4();
    sqlx::query("INSERT INTO business_legal_entities(id,code,name,country_code,functional_currency) VALUES($1,'EMPTY_REPORT_LE','Empty report legal entity','CN','CNY')").bind(empty).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_legal_entity_scopes(enterprise_user_id,legal_entity_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(empty).execute(pool).await.unwrap();
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
        };
        assert!(serde_json::to_value(&base)
            .unwrap()
            .get("legalEntityIds")
            .is_none());
        let full = service
            .operating_snapshot_preview(f.actor, &base)
            .await
            .unwrap();
        assert_eq!(full["schemaVersion"], 1);
        assert!(full["metrics"]["salesOrderCount"].as_i64().unwrap() > 0);
        assert!(
            full["metrics"]["inventoryValueAsOfGeneration"]
                .as_str()
                .unwrap()
                .parse::<Decimal>()
                .unwrap()
                > Decimal::ZERO
        );
        let legacy = service
            .generate_operating_snapshot(
                f.actor,
                Uuid::new_v4(),
                &format!("legal-legacy-{cadence}"),
                &base,
            )
            .await
            .unwrap();
        for selected in [f.legal_entity, empty] {
            let input = GenerateOperatingSnapshot {
                legal_entity_ids: Some(vec![selected]),
                ..base.clone()
            };
            let preview = service
                .operating_snapshot_preview(f.actor, &input)
                .await
                .unwrap();
            assert_eq!(preview["schemaVersion"], 2);
            assert_eq!(preview["scope"]["legalEntityIds"], json!([selected]));
            assert_ne!(preview["scopeHash"], full["scopeHash"]);
            if selected == empty {
                for dimension in [
                    "businessUnitIds",
                    "warehouseIds",
                    "customerIds",
                    "supplierIds",
                ] {
                    assert_eq!(preview["scope"][dimension], json!([]));
                }
            }
            assert_ne!(preview["dataQualityStatus"], "complete");
            for key in [
                "incidentsOpened",
                "incidentsResolved",
                "slaBreached",
                "averageResolutionHours",
            ] {
                assert!(preview["metrics"][key].is_null());
                assert_eq!(
                    preview["metrics"]["unavailableMetrics"][key],
                    "not_attributable_to_selected_legal_entities"
                );
            }
            for key in [
                "salesOrderAmount",
                "shippedRevenue",
                "purchaseOrderAmount",
                "inventoryValueAsOfGeneration",
                "managementOperatingProfit",
            ] {
                let amount = preview["metrics"][key]
                    .as_str()
                    .unwrap()
                    .parse::<Decimal>()
                    .unwrap();
                assert_eq!(
                    amount,
                    if selected == empty {
                        Decimal::ZERO
                    } else {
                        full["metrics"][key]
                            .as_str()
                            .unwrap()
                            .parse::<Decimal>()
                            .unwrap()
                    },
                    "{key}"
                );
            }
            let key = format!("legal-filter-{cadence}-{selected}");
            let saved = service
                .generate_operating_snapshot_guarded(
                    f.actor,
                    Uuid::new_v4(),
                    &key,
                    &input,
                    &preview,
                )
                .await
                .unwrap();
            assert_ne!(saved["id"], legacy["id"]);
            assert_eq!(saved["sourceHash"], preview["sourceHash"]);
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
                saved
            );
            let detail = service
                .operating_snapshot_detail(f.actor, saved["id"].as_str().unwrap().parse().unwrap())
                .await
                .unwrap();
            assert_eq!(detail["scope"], preview["scope"]);
            assert_eq!(detail["metrics"], preview["metrics"]);
        }
        let all = GenerateOperatingSnapshot {
            legal_entity_ids: Some(vec![empty, f.legal_entity, empty]),
            ..base.clone()
        };
        let all_preview = service
            .operating_snapshot_preview(f.actor, &all)
            .await
            .unwrap();
        assert_eq!(all_preview["metrics"]["unavailableMetrics"], json!({}));
        assert_eq!(
            all_preview["metrics"]["incidentsOpened"],
            full["metrics"]["incidentsOpened"]
        );
        for ids in [vec![], vec![Uuid::new_v4()]] {
            let invalid = GenerateOperatingSnapshot {
                legal_entity_ids: Some(ids),
                ..base.clone()
            };
            assert!(service
                .operating_snapshot_preview(f.actor, &invalid)
                .await
                .is_err());
        }
        let replay = service
            .generate_operating_snapshot(
                f.actor,
                Uuid::new_v4(),
                &format!("legal-legacy-{cadence}"),
                &base,
            )
            .await
            .unwrap();
        assert_eq!(replay, legacy);
        let stored: Value =
            sqlx::query_scalar("SELECT payload FROM operating_report_snapshots WHERE id=$1")
                .bind(legacy["id"].as_str().unwrap().parse::<Uuid>().unwrap())
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(stored, full["metrics"]);
    }
    sqlx::query("DELETE FROM business_legal_entity_scopes WHERE enterprise_user_id=$1 AND legal_entity_id=$2").bind(f.actor).bind(empty).execute(pool).await.unwrap();
}
