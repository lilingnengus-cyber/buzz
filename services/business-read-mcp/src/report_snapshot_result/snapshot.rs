use super::*;
fn hash(v: &Value) -> Option<String> {
    Some(
        Sha256::digest(serde_json::to_vec(v).ok()?)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
fn valid_snapshot(v: &Value, kind: &str) -> bool {
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
        && serde_json::from_value::<report_snapshot_tools::ReportSnapshotInput>(v["input"].clone())
            .is_ok()
        && v["ruleVersion"] == "management-profit-v1"
        && v["boundary"] == "not_statutory_financial_statement"
        && scope.as_object().is_some_and(|o| o.len() == 5)
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
                    && r["amount"].as_str().is_some_and(valid_decimal_string)
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

pub(super) fn validate(v: &Value, kind: &str) -> Result<(), String> {
    object(
        v,
        &[
            "schemaVersion",
            "kind",
            "input",
            "scope",
            "ruleVersion",
            "scopeHash",
            "sourceWatermark",
            "sourceHash",
            "components",
            "dataQualityStatus",
            "existingSnapshot",
            "effects",
            "boundary",
        ],
    )?;
    if !valid_snapshot(v, kind) {
        return Err("Invalid report snapshot content or hash".into());
    }
    Ok(())
}
