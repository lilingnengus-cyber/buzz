use super::*;
fn hash(v: &Value) -> Option<String> {
    Some(
        Sha256::digest(serde_json::to_vec(v).ok()?)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
pub(super) fn valid_snapshot(v: &Value, kind: &str) -> bool {
    let scope = &v["scope"];
    let arrays = [
        "legalEntityIds",
        "customerIds",
        "brandIds",
        "businessUnitIds",
        "warehouseIds",
    ];
    let existing = &v["existingSnapshot"];
    kind == "management_report_snapshot_intent"
        && v["schemaVersion"] == 1
        && v["kind"] == "management_report_snapshot"
        && canonical("prepare_management_report_snapshot", &v["input"]).as_ref()
            == Some(&v["input"])
        && v["ruleVersion"] == "management-profit-v1"
        && v["boundary"] == "not_statutory_financial_statement"
        && scope.as_object().is_some_and(|o| {
            o.keys().all(|k| {
                arrays.contains(&k.as_str())
                    || matches!(
                        k.as_str(),
                        "includeUnassignedBrand" | "includeUnassignedWarehouse"
                    )
            })
        })
        && ["includeUnassignedBrand", "includeUnassignedWarehouse"]
            .iter()
            .all(|key| scope.get(*key).is_none_or(|v| *v == json!(false)))
        && arrays
            .iter()
            .all(|key| scope[*key].as_array().is_some_and(|a| a.iter().all(uuid)))
        && scope["legalEntityIds"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
        && hash(scope).is_some_and(|h| v["scopeHash"] == h)
        && v["sourceWatermark"].as_i64().is_some_and(|n| n >= 0)
        && v["components"].as_array().is_some_and(|a| {
            a.iter().all(|r| {
                r["metricType"].is_string()
                    && r["amount"]
                        .as_str()
                        .is_some_and(|s| s.parse::<rust_decimal::Decimal>().is_ok())
                    && r["factCount"].as_i64().is_some_and(|n| n > 0)
            })
        })
        && hash(&json!({"scope":scope,"watermark":v["sourceWatermark"],"amounts":v["components"]}))
            .is_some_and(|h| v["sourceHash"] == h)
        && matches!(
            v["dataQualityStatus"].as_str(),
            Some("complete" | "partial")
        )
        && v["effects"]["changesSourceDocuments"] == false
        && v["effects"]["createsImmutableSnapshot"] == json!(existing.is_null())
        && (existing.is_null()
            || (uuid(&existing["id"])
                && existing["version"].as_i64().is_some_and(|n| n > 0)
                && existing["number"].as_str().is_some_and(|s| !s.is_empty())))
}
pub(super) fn binds(v: &Value, input: &Value) -> bool {
    v["input"] == *input
}
pub(super) fn permits(v: &Value, scope: &AuthorizationScope, kind: &str) -> bool {
    valid_snapshot(v, kind) && permits_scope(&v["scope"], scope)
}
pub(crate) fn permits_scope(report_scope: &Value, scope: &AuthorizationScope) -> bool {
    let dimensions = [
        "legalEntityIds",
        "customerIds",
        "businessUnitIds",
        "brandIds",
        "warehouseIds",
    ];
    if !report_scope.as_object().is_some_and(|o| {
        o.keys().all(|k| {
            dimensions.contains(&k.as_str())
                || matches!(
                    k.as_str(),
                    "includeUnassignedBrand" | "includeUnassignedWarehouse"
                )
        })
    }) || !dimensions.iter().all(|k| {
        report_scope[*k]
            .as_array()
            .is_some_and(|a| a.iter().all(uuid))
    }) || !["includeUnassignedBrand", "includeUnassignedWarehouse"]
        .iter()
        .all(|k| report_scope.get(*k).is_none_or(|v| *v == json!(false)))
    {
        return false;
    }
    scope.supplier_ids.is_empty()
        && (scope.brand_ids.is_empty() || report_scope["includeUnassignedBrand"] == false)
        && (scope.warehouse_ids.is_empty() || report_scope["includeUnassignedWarehouse"] == false)
        && [
            ("legalEntityIds", &scope.legal_entity_ids),
            ("customerIds", &scope.customer_ids),
            ("businessUnitIds", &scope.business_unit_ids),
            ("brandIds", &scope.brand_ids),
            ("warehouseIds", &scope.warehouse_ids),
        ]
        .iter()
        .all(|(key, allowed)| {
            allowed.is_empty()
                || report_scope[*key].as_array().is_some_and(|ids| {
                    !ids.is_empty()
                        && ids
                            .iter()
                            .all(|id| id.as_str().is_some_and(|s| allowed.contains(s)))
                })
        })
}
