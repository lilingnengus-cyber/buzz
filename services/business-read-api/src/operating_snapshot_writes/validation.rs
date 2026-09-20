use super::*;
use chrono::{DateTime, Duration, Utc};
fn hash(v: &Value) -> Option<String> {
    Some(
        Sha256::digest(serde_json::to_vec(v).ok()?)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
fn digest(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
pub(super) fn timestamp(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| DateTime::parse_from_rfc3339(s).is_ok())
}
fn period(v: &Value) -> Option<()> {
    let input: business_core::s1::GenerateOperatingSnapshot =
        serde_json::from_value(v["input"].clone()).ok()?;
    let end = input
        .period_start
        .checked_add_signed(Duration::days(if input.cadence == "daily" { 1 } else { 7 }))?;
    if v["periodEnd"] != json!(end) {
        return None;
    }
    for (key, date) in [
        ("periodStartUtc", input.period_start),
        ("periodEndUtc", end),
    ] {
        let expected = date
            .and_hms_opt(0, 0, 0)?
            .checked_sub_signed(Duration::minutes(i64::from(input.utc_offset_minutes)))?;
        let actual = DateTime::parse_from_rfc3339(v[key].as_str()?).ok()?;
        if actual.with_timezone(&Utc) != DateTime::<Utc>::from_naive_utc_and_offset(expected, Utc) {
            return None;
        }
    }
    Some(())
}
pub(super) fn valid_snapshot(v: &Value, kind: &str) -> bool {
    let scope = &v["scope"];
    let arrays = [
        "legalEntityIds",
        "customerIds",
        "brandIds",
        "businessUnitIds",
        "warehouseIds",
        "supplierIds",
    ];
    let existing = &v["existingSnapshot"];
    let counts = [
        "salesOrderCount",
        "shipmentCount",
        "purchaseOrderCount",
        "stockoutCountAsOfGeneration",
        "incidentsOpened",
        "incidentsResolved",
        "slaBreached",
    ];
    let amounts = [
        "salesOrderAmount",
        "shippedRevenue",
        "purchaseOrderAmount",
        "inventoryValueAsOfGeneration",
        "managementOperatingProfit",
        "averageResolutionHours",
    ];
    kind == "operating_report_snapshot_intent" && v["schemaVersion"]==if v["input"].get("legalEntityIds").is_some() || v["input"].get("businessUnitIds").is_some(){2}else{1} && v["kind"]=="operating_report_snapshot"
        && canonical("prepare_operating_report_snapshot", &v["input"]).as_ref()==Some(&v["input"])
        && uuid(&v["ownerUserId"]) && v["timeBasis"]=="fixed_utc_offset" && period(v).is_some()
        && v["boundary"]=="business_operations_only_not_financial_accounting"
        && scope.as_object().is_some_and(|o| o.len()==6 && o.keys().all(|k| arrays.contains(&k.as_str())))
        && arrays.iter().all(|k| scope[*k].as_array().is_some_and(|a| a.iter().all(uuid)))
        && scope["legalEntityIds"].as_array().is_some_and(|a| !a.is_empty()) && digest(&v["scopeHash"])
        && selected_scope_binds(v, "legalEntityIds") && selected_scope_binds(v, "businessUnitIds")
        && valid_metrics(v,&counts,&amounts)
        && hash(&json!({"cadence":v["input"]["cadence"],"utcOffsetMinutes":v["input"]["utcOffsetMinutes"],"periodStartUtc":v["periodStartUtc"],"periodEndUtc":v["periodEndUtc"],"periodStart":v["input"]["periodStart"],"periodEnd":v["periodEnd"],"currency":v["input"]["currency"],"scopeHash":v["scopeHash"],"metrics":v["metrics"]})).is_some_and(|h| v["sourceHash"]==h)
        && matches!(v["dataQualityStatus"].as_str(),Some("complete"|"partial"|"blocked"))
        && v["effects"]["changesSourceDocuments"]==false
        && v["effects"]["createsImmutableSnapshot"]==json!(existing.is_null())
        && (existing.is_null() || (uuid(&existing["id"]) && timestamp(&existing["generatedAt"])))
}
pub(super) fn binds(v: &Value, input: &Value) -> bool {
    v["input"] == *input
}
pub(super) fn permits(v: &Value, scope: &AuthorizationScope, kind: &str) -> bool {
    // Mixed-domain metrics do not yet implement all IAM dimension filters.
    valid_snapshot(v, kind)
        && scope.customer_ids.is_empty()
        && scope.supplier_ids.is_empty()
        && scope.brand_ids.is_empty()
        && (scope.business_unit_ids.is_empty()
            || (v["input"].get("businessUnitIds").is_some()
                && v["scope"]["businessUnitIds"].as_array().is_some_and(|a| {
                    !a.is_empty()
                        && a.iter().all(|id| {
                            id.as_str()
                                .is_some_and(|s| scope.business_unit_ids.contains(s))
                        })
                })))
        && scope.warehouse_ids.is_empty()
        && (scope.legal_entity_ids.is_empty()
            || v["scope"]["legalEntityIds"].as_array().is_some_and(|a| {
                !a.is_empty()
                    && a.iter().all(|id| {
                        id.as_str()
                            .is_some_and(|s| scope.legal_entity_ids.contains(s))
                    })
            }))
}

fn selected_scope_binds(v: &Value, field: &str) -> bool {
    let Some(input) = v["input"].get(field) else {
        return true;
    };
    let Some(ids) = input.as_array() else {
        return false;
    };
    if ids.is_empty() || !ids.iter().all(uuid) {
        return false;
    }
    let wanted: std::collections::BTreeSet<_> = ids.iter().filter_map(Value::as_str).collect();
    let actual: std::collections::BTreeSet<_> = v["scope"][field]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    actual == wanted
}
fn valid_metrics(v: &Value, counts: &[&str], amounts: &[&str]) -> bool {
    let metrics = &v["metrics"];
    let filtered =
        v["input"].get("legalEntityIds").is_some() || v["input"].get("businessUnitIds").is_some();
    if metrics
        .as_object()
        .is_none_or(|o| o.len() != counts.len() + amounts.len() + usize::from(filtered))
    {
        return false;
    }
    let unavailable = metrics.get("unavailableMetrics");
    if filtered {
        let Some(map) = unavailable.and_then(Value::as_object) else {
            return false;
        };
        let by_unit = v["input"].get("businessUnitIds").is_some();
        if by_unit && map.len() != 4 {
            return false;
        }
        if !map.is_empty()
            && (map.len() != 4
                || v["dataQualityStatus"] == "complete"
                || !map.iter().all(|(k, r)| {
                    [
                        "incidentsOpened",
                        "incidentsResolved",
                        "slaBreached",
                        "averageResolutionHours",
                    ]
                    .contains(&k.as_str())
                        && r == if by_unit {
                            "not_attributable_to_selected_business_units"
                        } else {
                            "not_attributable_to_selected_legal_entities"
                        }
                }))
        {
            return false;
        }
    } else if unavailable.is_some() {
        return false;
    }
    let missing = |k: &str| unavailable.is_some_and(|u| u.get(k).is_some());
    counts.iter().all(|k| {
        if missing(k) {
            metrics[*k].is_null()
        } else {
            metrics[*k].as_i64().is_some_and(|n| n >= 0)
        }
    }) && amounts.iter().all(|k| {
        if missing(k) {
            metrics[*k].is_null()
        } else {
            metrics[*k]
                .as_str()
                .is_some_and(|s| s.parse::<rust_decimal::Decimal>().is_ok())
        }
    })
}
