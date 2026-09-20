use super::*;
use rust_decimal::Decimal;
use std::collections::BTreeSet;

fn hash(v: &Value) -> Option<String> {
    Some(
        Sha256::digest(serde_json::to_vec(v).ok()?)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
fn amount(v: &Value) -> Option<Decimal> {
    v.as_str()?.parse().ok()
}
const DIMENSIONS: [&str; 6] = [
    "legalEntityIds",
    "businessUnitIds",
    "warehouseIds",
    "customerIds",
    "supplierIds",
    "brandIds",
];
pub(super) fn binds(v: &Value, input: &Value) -> bool {
    v["input"] == *input
}
pub(super) fn valid_snapshot(v: &Value, kind: &str) -> bool {
    if kind != "operational_adjustment_post_intent" {
        return super::drafts::validate(v, kind).is_some();
    }
    validate(v, kind).is_some()
}
fn validate(v: &Value, kind: &str) -> Option<()> {
    let input = canonical("prepare_operational_adjustment_post", &v["input"])?;
    let wrapper = &v["allocationPreview"];
    let p = &wrapper["preview"];
    let batch = &p["batch"];
    if kind != "operational_adjustment_post_intent"
        || v["schemaVersion"] != 1
        || v["kind"] != "operational_adjustment_post"
        || !uuid(&v["ownerUserId"])
        || v["effects"] != json!({"postsAdjustment":true,"createsBatch":false})
        || v["boundary"] != "management_only_not_general_ledger"
        || p["boundary"] != v["boundary"]
        || p["schemaVersion"] != 1
        || p["kind"] != "operational_adjustment_allocation"
        || p["scope"] != v["scope"]
        || wrapper["previewHash"] != hash(p)?
        || batch["id"] != input["batchId"]
        || batch["version"] != input["expectedVersion"]
        || !matches!(batch["status"].as_str(), Some("draft" | "previewed"))
        || batch["adjustment_number"].as_str()?.is_empty()
        || !uuid(&batch["legal_entity_id"])
        || p["sourceWatermark"].as_i64()? < 0
    {
        return None;
    }
    let currency = batch["currency"].as_str()?;
    if currency.len() != 3 || !currency.bytes().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    for key in DIMENSIONS {
        let ids = v["scope"][key].as_array()?;
        if !ids.iter().all(uuid)
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
    if !v["scope"]["legalEntityIds"]
        .as_array()?
        .contains(&batch["legal_entity_id"])
    {
        return None;
    }
    let targets = p["targets"].as_array()?;
    let mut target_ids = BTreeSet::new();
    for t in targets {
        if !uuid(&t["id"])
            || t["version"].as_i64()? < 1
            || t["legalEntityId"] != batch["legal_entity_id"]
            || t["currency"] != batch["currency"]
            || !target_ids.insert(t["id"].as_str()?)
        {
            return None;
        }
        for (field, scope_key) in [
            ("customerId", "customerIds"),
            ("businessUnitId", "businessUnitIds"),
            ("brandId", "brandIds"),
        ] {
            if field == "brandId" && t[field].is_null() {
                continue;
            }
            if !uuid(&t[field]) || !v["scope"][scope_key].as_array()?.contains(&t[field]) {
                return None;
            }
        }
    }
    let lines = p["lines"].as_array()?;
    let allocations = p["allocations"].as_array()?;
    if lines.is_empty() || lines.len() != allocations.len() || target_ids.is_empty() {
        return None;
    }
    let mut line_ids = BTreeSet::new();
    let mut used_targets = BTreeSet::new();
    let mut total = Decimal::ZERO;
    let mut allocated = Decimal::ZERO;
    for (line, allocation) in lines.iter().zip(allocations) {
        if !uuid(&line["id"])
            || !line_ids.insert(line["id"].as_str()?)
            || line["batch_id"] != batch["id"]
            || line["currency"] != batch["currency"]
            || line["legal_entity_id"] != batch["legal_entity_id"]
            || !matches!(
                line["metric_type"].as_str(),
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
            || !matches!(
                line["allocation_basis"].as_str(),
                Some(
                    "direct" | "net_revenue" | "product_cost" | "shipped_quantity" | "fixed_weight"
                )
            )
            || allocation["lineId"] != line["id"]
            || allocation["metricType"] != line["metric_type"]
            || allocation["businessDate"] != line["business_date"]
        {
            return None;
        }
        chrono::NaiveDate::parse_from_str(line["business_date"].as_str()?, "%Y-%m-%d").ok()?;
        let line_amount = amount(&line["amount"])?;
        if line_amount <= Decimal::ZERO || line_amount.round_dp(2) != line_amount {
            return None;
        }
        total = total.checked_add(line_amount)?;
        let mut line_total = Decimal::ZERO;
        let mut unique = BTreeSet::new();
        let mut weights = Decimal::ZERO;
        for t in allocation["targets"].as_array()? {
            let id = t["salesOrderId"].as_str()?;
            if !target_ids.contains(id)
                || !unique.insert(id)
                || amount(&t["weight"])? < Decimal::ZERO
                || t["remainderRank"].as_i64()? < 0
            {
                return None;
            }
            let allocated_amount = amount(&t["amount"])?;
            if allocated_amount < Decimal::ZERO || allocated_amount.round_dp(2) != allocated_amount
            {
                return None;
            }
            weights = weights.checked_add(amount(&t["weight"])?)?;
            used_targets.insert(id);
            line_total = line_total.checked_add(amount(&t["amount"])?)?;
        }
        if weights <= Decimal::ZERO || line_total != line_amount {
            return None;
        }
        allocated = allocated.checked_add(line_total)?;
    }
    if target_ids != used_targets
        || total != amount(&p["totalAmount"])?
        || allocated != amount(&p["allocatedAmount"])?
        || amount(&p["unallocatedAmount"])? != Decimal::ZERO
        || total != allocated
    {
        return None;
    }
    Some(())
}
pub(super) fn permits(v: &Value, scope: &AuthorizationScope, kind: &str) -> bool {
    if !valid_snapshot(v, kind) {
        return false;
    }
    // Core freezes the requester's complete scope. Require that scope to fit the
    // delegated grant; never widen or silently recompute a narrower allocation.
    for (key, allowed) in [
        ("legalEntityIds", &scope.legal_entity_ids),
        ("businessUnitIds", &scope.business_unit_ids),
        ("warehouseIds", &scope.warehouse_ids),
        ("customerIds", &scope.customer_ids),
        ("supplierIds", &scope.supplier_ids),
        ("brandIds", &scope.brand_ids),
    ] {
        if !allowed.is_empty()
            && v["scope"][key].as_array().is_none_or(|ids| {
                ids.iter()
                    .any(|id| id.as_str().is_none_or(|id| !allowed.contains(id)))
            })
        {
            return false;
        }
    }
    if kind != "operational_adjustment_post_intent" {
        return super::drafts::attributed(v, scope);
    }
    // A brand-limited grant cannot authorize facts without a brand attribution.
    scope.brand_ids.is_empty()
        || v["allocationPreview"]["preview"]["targets"]
            .as_array()
            .is_some_and(|ts| {
                ts.iter().all(|t| {
                    t["brandId"]
                        .as_str()
                        .is_some_and(|id| scope.brand_ids.contains(id))
                })
            })
}
