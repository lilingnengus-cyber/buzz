//! Validate order-hold effects and links before reporting execution to the human.
use super::*;
use sha2::{Digest, Sha256};

pub(super) fn family(tool: &str) -> Option<&'static str> {
    match tool {
        "prepare_sales_order_hold" | "approve_sales_order_hold" => Some("sales_order_hold_intent"),
        "prepare_sales_order_release_hold" | "approve_sales_order_release_hold" => {
            Some("sales_order_release_hold_intent")
        }
        _ => None,
    }
}
fn object(v: &Value, keys: &[&str]) -> Result<(), String> {
    if v.as_object()
        .is_none_or(|o| o.keys().any(|k| !keys.contains(&k.as_str())))
    {
        return Err("Unexpected order hold response fields".into());
    }
    Ok(())
}
fn uuid(v: &Value) -> bool {
    v.as_str().is_some_and(|s| s.parse::<Uuid>().is_ok())
}
fn positive(v: &Value) -> bool {
    v.as_i64().is_some_and(|v| v > 0)
}
fn bounded(v: &Value, context: &DelegationContext, max: usize) -> Result<(), String> {
    if serde_json::to_vec(v)
        .map_err(|_| "Invalid order hold response")?
        .len()
        > max
        || v["traceId"] != json!(context.trace_id)
    {
        return Err("Order hold response size or trace mismatch".into());
    }
    validate_business_value(v)
}
fn references(v: &Value, id: &Value) -> Result<(), String> {
    if !uuid(id) {
        return Err("Invalid order reference".into());
    }
    let refs = v.as_array().ok_or("Missing order hold reference")?;
    if refs.len() != 1 {
        return Err("Expected exactly one order reference".into());
    }
    let r = &refs[0];
    object(r, &["type", "id", "title", "bizUri"])?;
    if r["type"] != "sales_order"
        || r["id"] != *id
        || r["bizUri"]
            != format!(
                "biz://sales-order/{}",
                id.as_str().ok_or("Invalid order ID")?
            )
        || r["title"].as_str().is_none_or(str::is_empty)
    {
        return Err("Order hold link does not match the affected order".into());
    }
    Ok(())
}
fn snapshot(v: &Value, kind: &str) -> Result<(), String> {
    object(
        v,
        &[
            "source",
            "lines",
            "operation",
            "expectedVersion",
            "reasonCode",
            "targetHoldStatus",
            "blocksShipmentCreationAndConfirmation",
            "changesInventoryReservation",
            "canExecute",
        ],
    )?;
    let source = &v["source"];
    object(
        source,
        &[
            "id",
            "orderNumber",
            "legalEntityId",
            "customerId",
            "businessUnitId",
            "brandId",
            "version",
            "lifecycleStatus",
            "holdStatus",
        ],
    )?;
    let place = kind == "sales_order_hold_intent";
    let before = if place { "none" } else { "manual_review_hold" };
    let target = if place { "manual_review_hold" } else { "none" };
    if !["id", "legalEntityId", "customerId", "businessUnitId"]
        .iter()
        .all(|k| uuid(&source[*k]))
        || !(source["brandId"].is_null() || uuid(&source["brandId"]))
        || !positive(&source["version"])
        || !positive(&v["expectedVersion"])
        || source["orderNumber"].as_str().is_none_or(str::is_empty)
        || !matches!(
            source["lifecycleStatus"].as_str(),
            Some("draft" | "confirmed" | "completed" | "cancelled")
        )
        || !matches!(
            source["holdStatus"].as_str(),
            Some("none" | "manual_review_hold")
        )
        || v["reasonCode"]
            .as_str()
            .is_none_or(|s| s.trim().is_empty() || s.len() > 64)
        || v["operation"] != if place { "place_hold" } else { "release_hold" }
        || v["targetHoldStatus"] != target
        || v["blocksShipmentCreationAndConfirmation"] != place
        || v["changesInventoryReservation"] != false
        || v["canExecute"]
            != json!(
                source["lifecycleStatus"] == "confirmed"
                    && source["holdStatus"] == before
                    && source["version"] == v["expectedVersion"]
            )
    {
        return Err("Order hold preview state or effects are inconsistent".into());
    }
    let lines = v["lines"]
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or("Missing order lines")?;
    for line in lines {
        object(
            line,
            &["id", "skuId", "warehouseId", "businessUnitId", "brandId"],
        )?;
        if !["id", "skuId", "warehouseId", "businessUnitId"]
            .iter()
            .all(|k| uuid(&line[*k]))
            || !(line["brandId"].is_null() || uuid(&line["brandId"]))
        {
            return Err("Invalid order hold line scope".into());
        }
    }
    Ok(())
}
pub(super) fn prepare(
    tool: &str,
    v: &Value,
    c: &DelegationContext,
    max: usize,
) -> Result<(), String> {
    let kind = family(tool).ok_or("Unknown hold tool")?;
    bounded(v, c, max)?;
    object(
        v,
        &[
            "schemaVersion",
            "status",
            "traceId",
            "documentType",
            "item",
            "document",
            "previewHash",
            "approvalCommand",
            "rejectionCommand",
            "resourceRefs",
        ],
    )?;
    object(&v["item"], &["id", "version", "status"])?;
    if v["schemaVersion"] != 1
        || v["status"] != "ok"
        || v["documentType"] != kind
        || !uuid(&v["item"]["id"])
        || v["item"]["version"] != 1
        || v["item"]["status"] != "draft"
    {
        return Err("Invalid order hold intent envelope".into());
    }
    snapshot(&v["document"], kind)?;
    let hash = hex::encode(Sha256::digest(
        serde_json::to_vec(&v["document"]).map_err(|_| "Invalid hold preview")?,
    ));
    let id = v["item"]["id"].as_str().ok_or("Invalid intent ID")?;
    if v["previewHash"] != hash
        || v["approvalCommand"] != format!("确认 {} {id} v1 {hash}", kind.replace('_', "-"))
        || v["rejectionCommand"] != format!("拒绝 {} {id} v1 {hash}", kind.replace('_', "-"))
    {
        return Err("Order hold confirmation does not bind the preview".into());
    }
    references(&v["resourceRefs"], &v["document"]["source"]["id"])
}
pub(super) fn approval(
    tool: &str,
    v: &Value,
    c: &DelegationContext,
    max: usize,
) -> Result<(), String> {
    let kind = family(tool).ok_or("Unknown hold tool")?;
    bounded(v, c, max)?;
    object(
        v,
        &[
            "documentId",
            "documentType",
            "requestId",
            "status",
            "executed",
            "createdDocument",
            "approvalCount",
            "minimumApprovers",
            "traceId",
            "resourceRefs",
            "preview",
            "previewHash",
        ],
    )?;
    if v["documentType"] != kind
        || c.approval_document_type.as_deref() != Some(kind)
        || c.approval_expected_version != Some(1)
        || c.approval_document_id.is_none()
        || v["documentId"] != json!(c.approval_document_id)
        || !uuid(&v["requestId"])
    {
        return Err("Order hold approval does not match the signed intent".into());
    }
    snapshot(&v["preview"], kind)?;
    let hash = hex::encode(Sha256::digest(
        serde_json::to_vec(&v["preview"]).map_err(|_| "Invalid approved preview")?,
    ));
    if c.approval_preview_hash.as_deref() != Some(hash.as_str()) || v["previewHash"] != hash {
        return Err("Order hold result does not prove the signed preview".into());
    }
    let count = v["approvalCount"]
        .as_i64()
        .filter(|v| *v >= 0)
        .ok_or("Invalid approval count")?;
    let minimum = v["minimumApprovers"]
        .as_i64()
        .filter(|v| *v > 0)
        .ok_or("Invalid approval threshold")?;
    if v["executed"] == true {
        let d = &v["createdDocument"];
        object(
            d,
            &[
                "id",
                "number",
                "status",
                "version",
                "traceId",
                "idempotentReplay",
            ],
        )?;
        if c.approval_decision.as_deref() != Some("approve")
            || count < minimum
            || v["status"] != "executed"
            || d["status"]
                != if kind == "sales_order_hold_intent" {
                    "manual_review_hold"
                } else {
                    "none"
                }
            || v["preview"]["canExecute"] != true
            || d["id"] != v["preview"]["source"]["id"]
            || d["version"].as_i64()
                != v["preview"]["source"]["version"]
                    .as_i64()
                    .and_then(|v| v.checked_add(1))
            || !positive(&d["version"])
            || d["traceId"] != json!(c.trace_id)
            || !d["idempotentReplay"].is_boolean()
            || d["number"].as_str().is_none_or(str::is_empty)
        {
            return Err("Invalid executed order hold result".into());
        }
        references(&v["resourceRefs"], &d["id"])
    } else {
        let status = match c.approval_decision.as_deref() {
            Some("approve") => "pending",
            Some("reject") => "rejected",
            _ => return Err("Invalid signed decision".into()),
        };
        if v["executed"] != false
            || v["status"] != status
            || !v["createdDocument"].is_null()
            || v["resourceRefs"] != json!([])
            || count >= minimum
        {
            return Err("Unexecuted order hold result claims an effect".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
