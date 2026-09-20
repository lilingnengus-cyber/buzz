//! Validate report snapshot effects and links before reporting execution to the human.
use super::*;
use sha2::{Digest, Sha256};

pub(super) fn family(tool: &str) -> Option<&'static str> {
    match tool {
        "prepare_operating_report_snapshot" | "approve_operating_report_snapshot" => {
            Some("operating_report_snapshot_intent")
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
    if v["document"]["ownerUserId"] != json!(c.enterprise_user_id) {
        return Err("Operating report requester does not match delegation".into());
    }
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
    if v["resourceRefs"] != json!([]) {
        return Err("Operating snapshot detail links are unavailable".into());
    }
    Ok(())
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
                "created",
                "ownerUserId",
                "utcOffsetMinutes",
                "generatedAt",
                "sourceHash",
                "dataQualityStatus",
                "traceId",
            ],
        )?;
        let preview = &v["preview"];
        let existing = &preview["existingSnapshot"];
        if c.approval_decision.as_deref() != Some("approve")
            || count < minimum
            || v["status"] != "executed"
            || !uuid(&d["id"])
            || d["traceId"] != json!(c.trace_id)
            || d["ownerUserId"] != preview["ownerUserId"]
            || d["sourceHash"] != preview["sourceHash"]
            || d["dataQualityStatus"] != preview["dataQualityStatus"]
            || d["utcOffsetMinutes"] != preview["input"]["utcOffsetMinutes"]
            || d["created"] != json!(existing.is_null())
            || !snapshot::timestamp(&d["generatedAt"])
            || (!existing.is_null()
                && (d["id"] != existing["id"] || d["generatedAt"] != existing["generatedAt"]))
            || v["resourceRefs"] != json!([])
        {
            return Err("Invalid executed operating snapshot result".into());
        }
        Ok(())
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
