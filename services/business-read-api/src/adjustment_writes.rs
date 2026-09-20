//! Fixed adjustment-post preparations and independently verified signed approvals.
use super::*;
use inventory_count_writes::{bound_preview, fetch, fetch_bound};
use sha2::{Digest, Sha256};

pub(super) fn family(tool: &str) -> Option<&'static str> {
    match tool {
        "prepare_operational_adjustment_post" | "approve_operational_adjustment_post" => {
            Some("operational_adjustment_post_intent")
        }
        _ => None,
    }
}
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Prepare {
    batch_id: Uuid,
    expected_version: i64,
}
fn canonical(tool: &str, input: &Value) -> Option<Value> {
    if tool != "prepare_operational_adjustment_post" {
        return None;
    }
    let v: Prepare = serde_json::from_value(input.clone()).ok()?;
    if v.batch_id.is_nil() || v.expected_version < 1 {
        return None;
    }
    serde_json::to_value(v).ok()
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Approval {
    document_id: Uuid,
    expected_version: i64,
    preview_hash: String,
    decision: business_core::document_approval::ApprovalDecision,
}
pub(super) fn valid(tool: &str, input: &Value) -> bool {
    if family(tool).is_none() {
        return false;
    }
    if tool.starts_with("prepare_") {
        canonical(tool, input).is_some()
    } else {
        serde_json::from_value::<Approval>(input.clone()).is_ok_and(|v| {
            !v.document_id.is_nil()
                && v.expected_version == 1
                && v.preview_hash.len() == 64
                && v.preview_hash.bytes().all(|c| c.is_ascii_hexdigit())
        })
    }
}
fn uuid(value: &Value) -> bool {
    value.as_str().is_some_and(|s| s.parse::<Uuid>().is_ok())
}
mod validation;
use validation::{binds, permits, valid_snapshot};
pub(super) async fn forward(
    core: &CoreClient,
    tool: &str,
    input: Value,
    context: &RequestContext,
    grant: &EffectiveGrant,
) -> Response {
    let Some(kind) = family(tool) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(scope) = iam_authorization_scope(grant, &context.required_scope) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if tool.starts_with("prepare_") {
        let Some(input) = canonical(tool, &input) else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let preview = match fetch(
            core,
            &format!("v1/agent-adjustment-previews/{kind}"),
            Some(&input),
            context,
            tool,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => return e,
        };
        let snapshot = &preview["document"];
        if !valid_snapshot(snapshot, kind)
            || !binds(snapshot, &input)
            || snapshot["ownerUserId"] != json!(context.enterprise_user_id)
        {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        if !permits(snapshot, &scope, kind) {
            return StatusCode::FORBIDDEN.into_response();
        }
        let Ok(bytes) = serde_json::to_vec(snapshot) else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let preflight: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let mut prepared = match fetch_bound(
            core,
            &format!("v1/agent-adjustment-intents/{kind}"),
            Some(&input),
            context,
            tool,
            Some(&preflight),
        )
        .await
        {
            Ok(v) => v,
            Err(e) => return e,
        };
        if prepared["document"] != *snapshot {
            return StatusCode::CONFLICT.into_response();
        }
        if !bound_preview(&prepared, kind, context.trace_id) {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        if let Some(item) = prepared["item"].as_object_mut() {
            item.remove("snapshot");
        }
        prepared["item"]["status"] = json!("draft");
        prepared["schemaVersion"] = json!(1);
        prepared["status"] = json!("ok");
        prepared["documentType"] = json!(kind);
        prepared["resourceRefs"] = json!([]);
        return Json(prepared).into_response();
    }
    let Ok(command) = serde_json::from_value::<Approval>(input) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let id = command.document_id;
    let preview = match fetch(
        core,
        &format!("v1/agent-approval-previews/adjustments/{kind}/{id}"),
        None,
        context,
        tool,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !bound_preview(&preview, kind, context.trace_id) || preview["item"]["id"] != json!(id) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if command.expected_version != 1 || preview["previewHash"] != command.preview_hash {
        return StatusCode::CONFLICT.into_response();
    }
    if !permits(&preview["document"], &scope, kind) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let input = json!({"expectedVersion":1,"previewHash":command.preview_hash,"decision":command.decision,"sourceBuzzEventId":context.source_buzz_event_id,"sourceChannelId":context.source_channel_id});
    let mut result = match fetch(
        core,
        &format!("v1/agent-approvals/adjustments/{kind}/{id}"),
        Some(&input),
        context,
        tool,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return e,
    };
    if result["traceId"] != json!(context.trace_id)
        || result["documentId"] != json!(id)
        || result["documentType"] != kind
        || !result["executed"].is_boolean()
        || !uuid(&result["requestId"])
        || result["minimumApprovers"].as_i64().is_none_or(|n| n < 1)
        || result["approvalCount"].as_i64().is_none_or(|n| n < 0)
        || (result["executed"] == true
            && result["approvalCount"].as_i64() < result["minimumApprovers"].as_i64())
        || (result["status"] == "pending"
            && result["approvalCount"].as_i64() >= result["minimumApprovers"].as_i64())
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    result["resourceRefs"] = json!([]);
    if result["executed"] == true {
        let document = &result["postedDocument"];
        let batch = &preview["document"]["allocationPreview"]["preview"]["batch"];
        if document["id"] != batch["id"]
            || document["number"] != batch["adjustment_number"]
            || document["traceId"] != json!(context.trace_id)
            || document["status"] != "posted"
            || !document["idempotentReplay"].is_boolean()
            || batch["version"]
                .as_i64()
                .and_then(|v| v.checked_add(2))
                .is_none_or(|expected| document["version"].as_i64() != Some(expected))
            || result["status"] != "executed"
            || command.decision != business_core::document_approval::ApprovalDecision::Approve
        {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    if result["executed"] == false
        && (!result["postedDocument"].is_null()
            || result["status"]
                != if command.decision == business_core::document_approval::ApprovalDecision::Reject
                {
                    "rejected"
                } else {
                    "pending"
                })
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    result["preview"] = preview["document"].clone();
    result["previewHash"] = preview["previewHash"].clone();
    Json(result).into_response()
}

#[cfg(test)]
#[path = "adjustment_writes_tests.rs"]
mod tests;
