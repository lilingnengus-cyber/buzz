//! Validate adjustment posting effects and links before reporting execution to the human.
use super::*;
use sha2::{Digest, Sha256};

pub(super) fn family(tool: &str) -> Option<&'static str> {
    match tool {
        "prepare_operational_adjustment_reversal" | "approve_operational_adjustment_reversal" => {
            Some("operational_adjustment_reversal_intent")
        }
        "prepare_operational_adjustment_post" | "approve_operational_adjustment_post" => {
            Some("operational_adjustment_post_intent")
        }
        "prepare_operational_adjustment_creation" | "approve_operational_adjustment_creation" => {
            Some("operational_adjustment_creation_intent")
        }
        "prepare_operational_adjustment_update" | "approve_operational_adjustment_update" => {
            Some("operational_adjustment_update_intent")
        }
        _ => None,
    }
}
fn object(v: &Value, keys: &[&str]) -> Result<(), String> {
    if v.as_object()
        .is_none_or(|o| o.keys().any(|k| !keys.contains(&k.as_str())))
    {
        return Err("Unexpected adjustment posting response fields".into());
    }
    Ok(())
}
fn uuid(v: &Value) -> bool {
    v.as_str().is_some_and(|s| s.parse::<Uuid>().is_ok())
}
fn bounded(v: &Value, context: &DelegationContext, max: usize) -> Result<(), String> {
    if serde_json::to_vec(v)
        .map_err(|_| "Invalid adjustment posting response")?
        .len()
        > max
        || v["traceId"] != json!(context.trace_id)
    {
        return Err("Adjustment posting response size or trace mismatch".into());
    }
    // Core allocation rows inherit their currency from the batch. Validate that
    // convention on a copy; preserve the original bytes used by signed hashes.
    let mut monetary = v.clone();
    for field in ["document", "preview"] {
        let Some(p) = monetary
            .get_mut(field)
            .and_then(|v| v.get_mut("allocationPreview"))
            .and_then(|v| v.get_mut("preview"))
        else {
            continue;
        };
        let currency = p["batch"]["currency"].clone();
        if let Some(lines) = p.get_mut("allocations").and_then(Value::as_array_mut) {
            for line in lines {
                if let Some(targets) = line.get_mut("targets").and_then(Value::as_array_mut) {
                    for target in targets {
                        if let Some(target) = target.as_object_mut() {
                            if target.contains_key("currency") {
                                return Err("Unexpected allocation currency override".into());
                            }
                            target.insert("currency".into(), currency.clone());
                        }
                    }
                }
            }
        }
    }
    for field in ["document", "preview"] {
        for path in ["/input", "/input/batch", "/draftPreview/preview/input"] {
            if let Some(batch) = monetary.get_mut(field).and_then(|v| v.pointer_mut(path)) {
                let currency = batch["currency"].clone();
                if let Some(lines) = batch.get_mut("lines").and_then(Value::as_array_mut) {
                    for line in lines {
                        if let Some(line) = line.as_object_mut() {
                            if line.contains_key("currency") {
                                return Err("Unexpected draft line currency override".into());
                            }
                            line.insert("currency".into(), currency.clone());
                        }
                    }
                }
            }
        }
    }
    validate_business_value(&monetary)
}
mod drafts;
pub(super) mod reversal;
mod snapshot;
pub(super) fn requires_canonical_input(tool: &str) -> bool {
    family(tool).is_some_and(|k| k != "operational_adjustment_post_intent")
}
pub(super) fn draft_input(tool: &str, v: &Value) -> Option<Value> {
    if tool == "prepare_operational_adjustment_reversal" {
        reversal::canonical(v)
    } else {
        drafts::canonical(tool, v)
    }
}
fn snapshot(v: &Value, kind: &str) -> Result<(), String> {
    if kind == "operational_adjustment_post_intent" {
        snapshot::validate(v, kind)
    } else if kind == "operational_adjustment_reversal_intent" {
        reversal::validate(v).ok_or_else(|| "Invalid adjustment reversal snapshot".into())
    } else {
        drafts::validate(v, kind).ok_or_else(|| "Invalid adjustment draft snapshot".into())
    }
}
pub(super) fn prepare(
    tool: &str,
    v: &Value,
    c: &DelegationContext,
    max: usize,
) -> Result<(), String> {
    let kind = family(tool).ok_or("Unknown adjustment posting tool")?;
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
        return Err("Invalid adjustment posting intent envelope".into());
    }
    snapshot(&v["document"], kind)?;
    if v["document"]["ownerUserId"] != json!(c.enterprise_user_id) {
        return Err("Adjustment requester does not match delegation".into());
    }
    let hash = hex::encode(Sha256::digest(
        serde_json::to_vec(&v["document"]).map_err(|_| "Invalid adjustment preview")?,
    ));
    let id = v["item"]["id"].as_str().ok_or("Invalid intent ID")?;
    if v["previewHash"] != hash
        || v["approvalCommand"] != format!("确认 {} {id} v1 {hash}", kind.replace('_', "-"))
        || v["rejectionCommand"] != format!("拒绝 {} {id} v1 {hash}", kind.replace('_', "-"))
    {
        return Err("Adjustment posting confirmation does not bind the preview".into());
    }
    if v["resourceRefs"] != json!([]) {
        return Err("Unexpected adjustment detail link".into());
    }
    Ok(())
}

pub(super) fn approval(
    tool: &str,
    v: &Value,
    c: &DelegationContext,
    max: usize,
) -> Result<(), String> {
    let kind = family(tool).ok_or("Unknown adjustment posting tool")?;
    bounded(v, c, max)?;
    object(
        v,
        &[
            "documentId",
            "documentType",
            "requestId",
            "status",
            "executed",
            drafts::result_field(kind),
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
        return Err("Adjustment posting approval does not match the signed intent".into());
    }
    snapshot(&v["preview"], kind)?;
    let hash = hex::encode(Sha256::digest(
        serde_json::to_vec(&v["preview"]).map_err(|_| "Invalid approved preview")?,
    ));
    if c.approval_preview_hash.as_deref() != Some(hash.as_str()) || v["previewHash"] != hash {
        return Err("Adjustment posting result does not prove the signed preview".into());
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
        let d = &v[drafts::result_field(kind)];
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
        if kind != "operational_adjustment_post_intent" {
            if c.approval_decision.as_deref() != Some("approve")
                || count < minimum
                || v["resourceRefs"] != json!([])
                || !(if kind == "operational_adjustment_reversal_intent" {
                    reversal::valid_result(v, &v["preview"], c.trace_id)
                } else {
                    drafts::valid_result(v, &v["preview"], kind, c.trace_id)
                })
            {
                return Err("Invalid executed adjustment draft result".into());
            }
            return Ok(());
        }
        let batch = &v["preview"]["allocationPreview"]["preview"]["batch"];
        if c.approval_decision.as_deref() != Some("approve")
            || count < minimum
            || v["status"] != "executed"
            || d["id"] != batch["id"]
            || d["number"] != batch["adjustment_number"]
            || d["status"] != "posted"
            || d["traceId"] != json!(c.trace_id)
            || !d["idempotentReplay"].is_boolean()
            || batch["version"]
                .as_i64()
                .and_then(|v| v.checked_add(2))
                .is_none_or(|version| d["version"].as_i64() != Some(version))
            || v["resourceRefs"] != json!([])
        {
            return Err("Invalid executed adjustment posting result".into());
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
            || !v[drafts::result_field(kind)].is_null()
            || v["resourceRefs"] != json!([])
            || count >= minimum
        {
            return Err("Unexecuted adjustment posting result claims an effect".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod draft_tests;

#[cfg(test)]
mod reversal_tests;
