//! Independent validation of frozen historical facts, never a new allocation.
use super::*;
use rust_decimal::Decimal;
use std::collections::BTreeSet;
#[derive(serde::Deserialize, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Input {
    batch_id: Uuid,
    expected_version: i64,
    reason: String,
}
pub(super) fn canonical(v: &Value) -> Option<Value> {
    let v: Input = serde_json::from_value(v.clone()).ok()?;
    if v.batch_id.is_nil()
        || v.expected_version < 1
        || v.reason.trim().is_empty()
        || v.reason.chars().count() > 1000
        || v.reason
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return None;
    }
    serde_json::to_value(v).ok()
}
fn id(v: &Value) -> bool {
    v.as_str()
        .and_then(|s| s.parse::<Uuid>().ok())
        .is_some_and(|id| !id.is_nil())
}
fn decimal(v: &Value) -> Option<Decimal> {
    v.as_str()?.parse().ok()
}
pub(super) fn validate(v: &Value) -> Option<()> {
    let input = canonical(&v["input"])?;
    let wrapper = &v["reversalPreview"];
    let p = &wrapper["preview"];
    let b = &p["batch"];
    let hash: String = Sha256::digest(serde_json::to_vec(p).ok()?)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let effects = json!({"reversesAdjustment":true,"preservesOriginalFacts":true,"reallocatesAmounts":false,"bankRefund":false});
    if v["schemaVersion"] != 1
        || p["schemaVersion"] != 1
        || v["kind"] != "operational_adjustment_reversal"
        || p["kind"] != v["kind"]
        || !id(&v["ownerUserId"])
        || p["ownerUserId"] != v["ownerUserId"]
        || v["boundary"] != "management_only_not_general_ledger"
        || p["boundary"] != v["boundary"]
        || v["effects"] != effects
        || p["effects"] != effects
        || p["scope"] != v["scope"]
        || p["reason"] != input["reason"]
        || wrapper["previewHash"] != hash
        || b["id"] != input["batchId"]
        || b["version"] != input["expectedVersion"]
        || b["status"] != "posted"
        || b["adjustment_number"].as_str()?.is_empty()
        || p["currency"] != b["currency"]
    {
        return None;
    }
    let currency = b["currency"].as_str()?;
    if currency.len() != 3 || !currency.bytes().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    for key in [
        "legalEntityIds",
        "customerIds",
        "businessUnitIds",
        "warehouseIds",
        "supplierIds",
        "brandIds",
    ] {
        let ids = v["scope"][key].as_array()?;
        if !ids.iter().all(id)
            || ids
                .iter()
                .filter_map(Value::as_str)
                .collect::<BTreeSet<_>>()
                .len()
                != ids.len()
        {
            return None;
        }
    }
    if !id(&b["legal_entity_id"])
        || !v["scope"]["legalEntityIds"]
            .as_array()?
            .contains(&b["legal_entity_id"])
    {
        return None;
    }
    let rows = p["facts"].as_array()?;
    if rows.is_empty() {
        return None;
    }
    let mut facts = BTreeSet::new();
    let mut allocations = BTreeSet::new();
    let mut orders = BTreeSet::new();
    let mut total = Decimal::ZERO;
    for row in rows {
        let a = &row["allocation"];
        let f = &row["fact"];
        if !id(&a["id"])
            || !allocations.insert(a["id"].as_str()?)
            || !id(&f["id"])
            || !facts.insert(f["id"].as_str()?)
            || !id(&a["adjustment_line_id"])
            || !id(&a["preview_id"])
            || a["batch_id"] != b["id"]
            || f["source_id"] != b["id"]
            || f["source_line_id"] != a["id"]
            || a["profit_fact_id"] != f["id"]
            || f["source_type"] != "operational_adjustment"
            || f["direction"] != "normal"
            || f["currency"] != b["currency"]
            || f["legal_entity_id"] != b["legal_entity_id"]
            || !id(&f["sales_order_id"])
            || f["sales_order_id"] != a["sales_order_id"]
            || decimal(&a["weight"])? < Decimal::ZERO
            || a["remainder_rank"].as_i64()? < 0
            || !matches!(
                f["metric_type"].as_str(),
                Some(
                    "outbound_freight"
                        | "sales_commission"
                        | "platform_fee"
                        | "customer_rebate"
                        | "supplier_rebate"
                        | "other_direct_cost"
                        | "allocated_operating_expense"
                )
            )
        {
            return None;
        }
        chrono::NaiveDate::parse_from_str(f["business_date"].as_str()?, "%Y-%m-%d").ok()?;
        for (field, key) in [
            ("customer_id", "customerIds"),
            ("business_unit_id", "businessUnitIds"),
            ("brand_id", "brandIds"),
            ("warehouse_id", "warehouseIds"),
        ] {
            if !f[field].is_null()
                && (!id(&f[field]) || !v["scope"][key].as_array()?.contains(&f[field]))
            {
                return None;
            }
        }
        let amount = decimal(&f["amount"])?;
        if amount < Decimal::ZERO
            || amount.round_dp(2) != amount
            || decimal(&a["allocated_amount"])? != amount
        {
            return None;
        }
        if !f["quantity"].is_null() {
            decimal(&f["quantity"])?;
        }
        total = total.checked_add(amount)?;
        orders.insert(f["sales_order_id"].as_str()?);
    }
    let targets = p["targetOrderIds"].as_array()?;
    let target_ids = targets
        .iter()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    if !targets.iter().all(id)
        || target_ids.len() != targets.len()
        || orders != target_ids
        || total <= Decimal::ZERO
        || total != decimal(&p["totalAmount"])?
    {
        return None;
    }
    Some(())
}
pub(super) fn valid_result(r: &Value, v: &Value, trace: Uuid) -> bool {
    let d = &r["reversedDocument"];
    let b = &v["reversalPreview"]["preview"]["batch"];
    r["status"] == "executed"
        && d["id"] == b["id"]
        && d["number"] == b["adjustment_number"]
        && d["status"] == "reversed"
        && d["traceId"] == json!(trace)
        && d["idempotentReplay"].is_boolean()
        && b["version"]
            .as_i64()
            .and_then(|v| v.checked_add(1))
            .is_some_and(|n| d["version"].as_i64() == Some(n))
}
