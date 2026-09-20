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
    let input: operating_snapshot_tools::OperatingSnapshotInput =
        serde_json::from_value(v["input"].clone()).ok()?;
    let start = chrono::NaiveDate::parse_from_str(&input.period_start, "%Y-%m-%d").ok()?;
    let end =
        start.checked_add_signed(Duration::days(if input.cadence == "daily" { 1 } else { 7 }))?;
    if v["periodEnd"] != json!(end) {
        return None;
    }
    for (key, date) in [("periodStartUtc", start), ("periodEndUtc", end)] {
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
    kind == "operating_report_snapshot_intent" && v["schemaVersion"]==1 && v["kind"]=="operating_report_snapshot"
        && canonical("prepare_operating_report_snapshot", &v["input"]).as_ref()==Some(&v["input"])
        && uuid(&v["ownerUserId"]) && v["timeBasis"]=="fixed_utc_offset" && period(v).is_some()
        && v["boundary"]=="business_operations_only_not_financial_accounting"
        && scope.as_object().is_some_and(|o| o.len()==6 && o.keys().all(|k| arrays.contains(&k.as_str())))
        && arrays.iter().all(|k| scope[*k].as_array().is_some_and(|a| a.iter().all(uuid)))
        && scope["legalEntityIds"].as_array().is_some_and(|a| !a.is_empty()) && digest(&v["scopeHash"])
        && v["metrics"].as_object().is_some_and(|o| o.len()==counts.len()+amounts.len())
        && counts.iter().all(|k| v["metrics"][*k].as_i64().is_some_and(|n| n>=0))
        && amounts.iter().all(|k| v["metrics"][*k].as_str().is_some_and(valid_decimal_string))
        && hash(&json!({"cadence":v["input"]["cadence"],"utcOffsetMinutes":v["input"]["utcOffsetMinutes"],"periodStartUtc":v["periodStartUtc"],"periodEndUtc":v["periodEndUtc"],"periodStart":v["input"]["periodStart"],"periodEnd":v["periodEnd"],"currency":v["input"]["currency"],"scopeHash":v["scopeHash"],"metrics":v["metrics"]})).is_some_and(|h| v["sourceHash"]==h)
        && matches!(v["dataQualityStatus"].as_str(),Some("complete"|"partial"|"blocked"))
        && v["effects"]["changesSourceDocuments"]==false
        && v["effects"]["createsImmutableSnapshot"]==json!(existing.is_null())
        && (existing.is_null() || (uuid(&existing["id"]) && timestamp(&existing["generatedAt"])))
}

fn canonical(_tool: &str, v: &Value) -> Option<Value> {
    use chrono::Datelike;
    let input: operating_snapshot_tools::OperatingSnapshotInput =
        serde_json::from_value(v.clone()).ok()?;
    let date = chrono::NaiveDate::parse_from_str(&input.period_start, "%Y-%m-%d").ok()?;
    if !matches!(input.cadence.as_str(), "daily" | "weekly")
        || (input.cadence == "weekly" && date.weekday() != chrono::Weekday::Mon)
        || !(-720..=840).contains(&input.utc_offset_minutes)
        || input.currency.len() != 3
        || !input.currency.bytes().all(|b| b.is_ascii_uppercase())
    {
        return None;
    }
    serde_json::to_value(input).ok()
}
pub(super) fn validate(v: &Value, kind: &str) -> Result<(), String> {
    object(
        v,
        &[
            "schemaVersion",
            "kind",
            "input",
            "ownerUserId",
            "periodEnd",
            "periodStartUtc",
            "periodEndUtc",
            "timeBasis",
            "scope",
            "scopeHash",
            "metrics",
            "sourceHash",
            "dataQualityStatus",
            "existingSnapshot",
            "effects",
            "boundary",
        ],
    )?;
    object(
        &v["effects"],
        &["createsImmutableSnapshot", "changesSourceDocuments"],
    )?;
    if !v["existingSnapshot"].is_null() {
        object(&v["existingSnapshot"], &["id", "generatedAt"])?;
    }
    if !valid_snapshot(v, kind) {
        return Err("Invalid operating snapshot content or hash".into());
    }
    Ok(())
}
