//! Independent draft input, frozen preview and mutation result validation.
use super::*;
use crate::adjustment_draft_inputs::CreateAdjustmentBatch;
use rust_decimal::Decimal;
use std::collections::BTreeSet;
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Replacement {
    batch_id: Uuid,
    expected_version: i64,
    batch: CreateAdjustmentBatch,
}
fn id(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| s.parse::<Uuid>().is_ok_and(|id| !id.is_nil()))
}
fn decimal(v: &Value) -> Option<Decimal> {
    v.as_str()?.parse().ok()
}
fn hash(v: &Value) -> Option<String> {
    Some(
        Sha256::digest(serde_json::to_vec(v).ok()?)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
pub(crate) fn canonical(tool: &str, v: &Value) -> Option<Value> {
    match tool {
        "prepare_operational_adjustment_creation" => {
            let input: CreateAdjustmentBatch = serde_json::from_value(v.clone()).ok()?;
            let mut value = serde_json::to_value(input).ok()?;
            normalize(&mut value)?;
            batch(&value)?;
            Some(value)
        }
        "prepare_operational_adjustment_update" => {
            let input: Replacement = serde_json::from_value(v.clone()).ok()?;
            if input.batch_id.is_nil() || input.expected_version < 1 {
                return None;
            }
            let mut value = serde_json::to_value(input).ok()?;
            normalize(&mut value["batch"])?;
            batch(&value["batch"])?;
            Some(value)
        }
        _ => None,
    }
}
fn batch(v: &Value) -> Option<Decimal> {
    if !id(&v["legalEntityId"]) {
        return None;
    }
    let currency = v["currency"].as_str()?;
    if currency.len() != 3 || !currency.bytes().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let period = v["managementPeriod"].as_str()?;
    if period.len() != 7 {
        return None;
    }
    chrono::NaiveDate::parse_from_str(&format!("{period}-01"), "%Y-%m-%d").ok()?;
    let lines = v["lines"].as_array()?;
    if lines.is_empty() || lines.len() > 200 {
        return None;
    }
    let mut total = Decimal::ZERO;
    for l in lines {
        let amount = decimal(&l["amount"])?;
        if amount <= Decimal::ZERO || amount.round_dp(2) != amount {
            return None;
        }
        total = total.checked_add(amount)?;
        if !matches!(
            l["metricType"].as_str(),
            Some(
                "outbound_freight"
                    | "sales_commission"
                    | "platform_fee"
                    | "customer_rebate"
                    | "supplier_rebate"
                    | "other_direct_cost"
                    | "allocated_operating_expense"
            )
        ) || !matches!(
            l["allocationBasis"].as_str(),
            Some("direct" | "net_revenue" | "product_cost" | "shipped_quantity" | "fixed_weight")
        ) {
            return None;
        }
        chrono::NaiveDate::parse_from_str(l["businessDate"].as_str()?, "%Y-%m-%d").ok()?;
        for key in [
            "directSalesOrderId",
            "customerId",
            "skuId",
            "brandId",
            "salespersonUserId",
            "businessUnitId",
            "departmentId",
            "warehouseId",
        ] {
            if !l[key].is_null() && !id(&l[key]) {
                return None;
            }
        }
        if l["allocationBasis"] == "direct" && !id(&l["directSalesOrderId"]) {
            return None;
        }
        let reason = l["reasonCode"].as_str()?;
        if reason.trim().is_empty()
            || reason.chars().count() > 256
            || reason.chars().any(char::is_control)
        {
            return None;
        }
        for (key, max) in [("sourceReference", 256), ("businessNote", 4096)] {
            if !l[key].is_null() && l[key].as_str()?.chars().count() > max {
                return None;
            }
        }
        let ids = l["salesOrderIds"].as_array()?;
        if ids.len() > 500
            || !ids.iter().all(id)
            || ids
                .iter()
                .filter_map(Value::as_str)
                .collect::<BTreeSet<_>>()
                .len()
                != ids.len()
        {
            return None;
        }
        let weights = l["fixedWeights"].as_array()?;
        if weights.len() > 500 || (l["allocationBasis"] != "fixed_weight" && !weights.is_empty()) {
            return None;
        }
        let mut weight_ids = BTreeSet::new();
        let mut weight_total = Decimal::ZERO;
        for w in weights {
            if !id(&w["salesOrderId"])
                || !weight_ids.insert(w["salesOrderId"].as_str()?)
                || decimal(&w["weight"])? < Decimal::ZERO
            {
                return None;
            }
            weight_total = weight_total.checked_add(decimal(&w["weight"])?)?;
        }
        if l["allocationBasis"] == "fixed_weight" && weight_total <= Decimal::ZERO {
            return None;
        }
    }
    if references(v)?.len() > 500 {
        return None;
    }
    Some(total)
}
fn references(v: &Value) -> Option<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    for l in v["lines"].as_array()? {
        if let Some(id) = l["directSalesOrderId"].as_str() {
            ids.insert(id.into());
        }
        for id in l["salesOrderIds"].as_array()? {
            ids.insert(id.as_str()?.into());
        }
        for w in l["fixedWeights"].as_array()? {
            ids.insert(w["salesOrderId"].as_str()?.into());
        }
    }
    Some(ids)
}
pub(super) fn result_field(kind: &str) -> &'static str {
    match kind {
        "operational_adjustment_creation_intent" => "createdDocument",
        "operational_adjustment_update_intent" => "updatedDocument",
        "operational_adjustment_reversal_intent" => "reversedDocument",
        _ => "postedDocument",
    }
}
pub(super) fn validate(v: &Value, kind: &str) -> Option<()> {
    let create = match kind {
        "operational_adjustment_creation_intent" => true,
        "operational_adjustment_update_intent" => false,
        _ => return None,
    };
    let input = canonical(
        if create {
            "prepare_operational_adjustment_creation"
        } else {
            "prepare_operational_adjustment_update"
        },
        &v["input"],
    )?;
    let input_batch = if create { &input } else { &input["batch"] };
    let p = &v["draftPreview"]["preview"];
    let effects = json!({"createsBatch":create,"replacesAllLines":!create,"postsAdjustment":false,"allocatesAmounts":false});
    if v["schemaVersion"] != 1
        || p["schemaVersion"] != 1
        || !id(&v["ownerUserId"])
        || p["ownerUserId"] != v["ownerUserId"]
        || v["kind"]
            != if create {
                "operational_adjustment_draft_create"
            } else {
                "operational_adjustment_draft_replace"
            }
        || p["kind"] != v["kind"]
        || v["effects"] != effects
        || p["effects"] != effects
        || v["boundary"] != "management_only_not_general_ledger"
        || p["boundary"] != v["boundary"]
        || p["scope"] != v["scope"]
        || p["input"] != *input_batch
        || v["draftPreview"]["previewHash"] != hash(p)?
        || p["currency"] != input_batch["currency"]
        || decimal(&p["totalAmount"])? != batch(input_batch)?
    {
        return None;
    }
    for key in [
        "legalEntityIds",
        "businessUnitIds",
        "warehouseIds",
        "customerIds",
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
    if !v["scope"]["legalEntityIds"]
        .as_array()?
        .contains(&input_batch["legalEntityId"])
    {
        return None;
    }
    let mut actual = BTreeSet::new();
    for o in p["referencedOrders"].as_array()? {
        if !id(&o["id"])
            || !actual.insert(o["id"].as_str()?.to_string())
            || o["version"].as_i64()? < 1
            || o["legalEntityId"] != input_batch["legalEntityId"]
            || o["currency"] != input_batch["currency"]
        {
            return None;
        }
        for (field, scope) in [
            ("customerId", "customerIds"),
            ("businessUnitId", "businessUnitIds"),
            ("brandId", "brandIds"),
        ] {
            if field == "brandId" && o[field].is_null() {
                continue;
            }
            if !id(&o[field]) || !v["scope"][scope].as_array()?.contains(&o[field]) {
                return None;
            }
        }
    }
    if actual != references(input_batch)? {
        return None;
    }
    for l in input_batch["lines"].as_array()? {
        for (field, scope) in [
            ("customerId", "customerIds"),
            ("businessUnitId", "businessUnitIds"),
            ("warehouseId", "warehouseIds"),
            ("brandId", "brandIds"),
        ] {
            if !l[field].is_null() && !v["scope"][scope].as_array()?.contains(&l[field]) {
                return None;
            }
        }
    }
    if create {
        if !p["source"].is_null() {
            return None;
        }
    } else {
        validate_source(&p["source"], &input, v)?;
    }
    Some(())
}
fn validate_source(s: &Value, input: &Value, v: &Value) -> Option<()> {
    let b = &s["batch"];
    if b["id"] != input["batchId"]
        || b["version"] != input["expectedVersion"]
        || !matches!(b["status"].as_str(), Some("draft" | "previewed"))
        || b["adjustment_number"].as_str()?.is_empty()
        || !v["scope"]["legalEntityIds"]
            .as_array()?
            .contains(&b["legal_entity_id"])
    {
        return None;
    }
    let lines = s["lines"].as_array()?;
    if lines.is_empty() || lines.len() > 200 {
        return None;
    }
    let mut total = Decimal::ZERO;
    let mut ids = BTreeSet::new();
    let mut numbers = BTreeSet::new();
    let mut typed_lines = Vec::new();
    for l in lines {
        if !id(&l["id"])
            || !ids.insert(l["id"].as_str()?)
            || l["batch_id"] != b["id"]
            || l["legal_entity_id"] != b["legal_entity_id"]
            || l["currency"] != b["currency"]
            || l["line_number"].as_i64()? < 1
            || !numbers.insert(l["line_number"].as_i64()?)
        {
            return None;
        }
        let amount = decimal(&l["amount"])?;
        if amount <= Decimal::ZERO || amount.round_dp(2) != amount {
            return None;
        }
        total = total.checked_add(amount)?;
        for (field, scope) in [
            ("customer_id", "customerIds"),
            ("business_unit_id", "businessUnitIds"),
            ("warehouse_id", "warehouseIds"),
            ("brand_id", "brandIds"),
        ] {
            if !l[field].is_null() && !v["scope"][scope].as_array()?.contains(&l[field]) {
                return None;
            }
        }
    }
    for l in lines {
        let mut line = serde_json::Map::new();
        for (camel, snake) in [
            ("metricType", "metric_type"),
            ("amount", "amount"),
            ("businessDate", "business_date"),
            ("allocationBasis", "allocation_basis"),
            ("directSalesOrderId", "direct_sales_order_id"),
            ("customerId", "customer_id"),
            ("skuId", "sku_id"),
            ("brandId", "brand_id"),
            ("salespersonUserId", "salesperson_user_id"),
            ("businessUnitId", "business_unit_id"),
            ("departmentId", "department_id"),
            ("warehouseId", "warehouse_id"),
            ("reasonCode", "reason_code"),
            ("sourceReference", "source_reference"),
            ("businessNote", "business_note"),
        ] {
            line.insert(camel.into(), l[snake].clone());
        }
        line.insert(
            "salesOrderIds".into(),
            l["allocation_scope"]["salesOrderIds"].clone(),
        );
        line.insert(
            "fixedWeights".into(),
            l["allocation_scope"]["fixedWeights"].clone(),
        );
        typed_lines.push(Value::Object(line));
    }
    let original = json!({"legalEntityId":b["legal_entity_id"],"currency":b["currency"],"managementPeriod":b["management_period"],"lines":typed_lines});
    if batch(&original)? != total {
        return None;
    }
    if total != decimal(&s["totalAmount"])? {
        return None;
    }
    Some(())
}
pub(super) fn valid_result(result: &Value, v: &Value, kind: &str, trace: Uuid) -> bool {
    let d = &result[result_field(kind)];
    if !id(&d["id"])
        || d["number"].as_str().is_none_or(str::is_empty)
        || d["status"] != "draft"
        || d["traceId"] != json!(trace)
        || !d["idempotentReplay"].is_boolean()
        || result["status"] != "executed"
    {
        return false;
    }
    if kind == "operational_adjustment_creation_intent" {
        d["version"] == 1
    } else {
        let b = &v["draftPreview"]["preview"]["source"]["batch"];
        d["id"] == b["id"]
            && d["number"] == b["adjustment_number"]
            && b["version"]
                .as_i64()
                .and_then(|x| x.checked_add(1))
                .is_some_and(|x| d["version"] == x)
    }
}

fn normalize(v: &mut Value) -> Option<()> {
    for line in v.get_mut("lines")?.as_array_mut()? {
        line["amount"] = json!(decimal(&line["amount"])?.to_string());
        let date =
            chrono::NaiveDate::parse_from_str(line["businessDate"].as_str()?, "%Y-%m-%d").ok()?;
        line["businessDate"] = json!(date.format("%Y-%m-%d").to_string());
        for weight in line.get_mut("fixedWeights")?.as_array_mut()? {
            weight["weight"] = json!(decimal(&weight["weight"])?.to_string());
        }
    }
    Some(())
}
