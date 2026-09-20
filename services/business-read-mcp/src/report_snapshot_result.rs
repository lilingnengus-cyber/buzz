//! Validate report snapshot effects and links before reporting execution to the human.
use super::*;
use sha2::{Digest, Sha256};

pub(super) fn family(tool: &str) -> Option<&'static str> {
    match tool {
        "prepare_management_report_snapshot" | "approve_management_report_snapshot" => {
            Some("management_report_snapshot_intent")
        }
        _ => None,
    }
}
fn object(v: &Value, keys: &[&str]) -> Result<(), String> {
    if v.as_object()
        .is_none_or(|o| o.keys().any(|k| !keys.contains(&k.as_str())))
    {
        return Err("Unexpected report snapshot response fields".into());
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
        .map_err(|_| "Invalid report snapshot response")?
        .len()
        > max
        || v["traceId"] != json!(context.trace_id)
    {
        return Err("Report snapshot response size or trace mismatch".into());
    }
    validate_business_value(v)
}
fn references(v: &Value, id: &Value) -> Result<(), String> {
    if !uuid(id) {
        return Err("Invalid report reference".into());
    }
    let refs = v.as_array().ok_or("Missing report snapshot reference")?;
    if refs.len() != 1 {
        return Err("Expected exactly one report reference".into());
    }
    let r = &refs[0];
    object(r, &["type", "id", "title", "bizUri"])?;
    if r["type"] != "management_report"
        || r["id"] != *id
        || r["bizUri"]
            != format!(
                "biz://management-report/{}",
                id.as_str().ok_or("Invalid order ID")?
            )
        || r["title"].as_str().is_none_or(str::is_empty)
    {
        return Err("Report snapshot link does not match the affected order".into());
    }
    Ok(())
}
mod snapshot;
use snapshot::validate as snapshot;
pub(super) fn prepare(
    tool: &str,
    v: &Value,
    c: &DelegationContext,
    max: usize,
) -> Result<(), String> {
    let kind = family(tool).ok_or("Unknown report snapshot tool")?;
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
        return Err("Invalid report snapshot intent envelope".into());
    }
    snapshot(&v["document"], kind)?;
    let hash = hex::encode(Sha256::digest(
        serde_json::to_vec(&v["document"]).map_err(|_| "Invalid report preview")?,
    ));
    let id = v["item"]["id"].as_str().ok_or("Invalid intent ID")?;
    if v["previewHash"] != hash
        || v["approvalCommand"] != format!("确认 {} {id} v1 {hash}", kind.replace('_', "-"))
        || v["rejectionCommand"] != format!("拒绝 {} {id} v1 {hash}", kind.replace('_', "-"))
    {
        return Err("Report snapshot confirmation does not bind the preview".into());
    }
    if v["document"]["existingSnapshot"].is_null() {
        if v["resourceRefs"] != json!([]) {
            return Err("New snapshot has no detail link before generation".into());
        }
        Ok(())
    } else {
        references(&v["resourceRefs"], &v["document"]["existingSnapshot"]["id"])
    }
}
pub(super) fn approval(
    tool: &str,
    v: &Value,
    c: &DelegationContext,
    max: usize,
) -> Result<(), String> {
    let kind = family(tool).ok_or("Unknown report snapshot tool")?;
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
        return Err("Report snapshot approval does not match the signed intent".into());
    }
    snapshot(&v["preview"], kind)?;
    let hash = hex::encode(Sha256::digest(
        serde_json::to_vec(&v["preview"]).map_err(|_| "Invalid approved preview")?,
    ));
    if c.approval_preview_hash.as_deref() != Some(hash.as_str()) || v["previewHash"] != hash {
        return Err("Report snapshot result does not prove the signed preview".into());
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
            || d["status"] != "generated"
            || !uuid(&d["id"])
            || !positive(&d["version"])
            || (!v["preview"]["existingSnapshot"].is_null()
                && (d["id"] != v["preview"]["existingSnapshot"]["id"]
                    || d["version"] != v["preview"]["existingSnapshot"]["version"]
                    || d["number"] != v["preview"]["existingSnapshot"]["number"]))
            || d["traceId"] != json!(c.trace_id)
            || !d["idempotentReplay"].is_boolean()
            || d["number"].as_str().is_none_or(str::is_empty)
        {
            return Err("Invalid executed report snapshot result".into());
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
            return Err("Unexecuted report snapshot result claims an effect".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
