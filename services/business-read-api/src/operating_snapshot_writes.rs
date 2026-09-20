//! Fixed report-snapshot preparations and independently verified signed approvals.
use super::*;
use inventory_count_writes::{bound_preview, fetch, fetch_bound};
use sha2::{Digest, Sha256};

pub(super) fn family(tool: &str) -> Option<&'static str> {
    match tool {
        "prepare_operating_report_snapshot" | "approve_operating_report_snapshot" => {
            Some("operating_report_snapshot_intent")
        }
        _ => None,
    }
}
fn canonical(tool: &str, input: &Value) -> Option<Value> {
    if tool != "prepare_operating_report_snapshot" {
        return None;
    }
    let v: business_core::s1::GenerateOperatingSnapshot =
        serde_json::from_value(input.clone()).ok()?;
    use chrono::Datelike;
    if !matches!(v.cadence.as_str(), "daily" | "weekly")
        || !(-720..=840).contains(&v.utc_offset_minutes)
        || v.currency.len() != 3
        || !v.currency.bytes().all(|c| c.is_ascii_uppercase())
        || (v.cadence == "weekly" && v.period_start.weekday() != chrono::Weekday::Mon)
    {
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
            v.expected_version == 1
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
fn resource(id: Uuid) -> Value {
    json!({"type":"operating_snapshot","id":id,"title":"查看经营快照","bizUri":format!("biz://operating-snapshot/{id}")})
}
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
            &format!("v1/agent-operating-snapshot-previews/{kind}"),
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
            &format!("v1/agent-operating-snapshot-intents/{kind}"),
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
        prepared["resourceRefs"] = snapshot["existingSnapshot"]["id"]
            .as_str()
            .and_then(|s| s.parse::<Uuid>().ok())
            .map_or_else(|| json!([]), |id| json!([resource(id)]));
        return Json(prepared).into_response();
    }
    let Ok(command) = serde_json::from_value::<Approval>(input) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let id = command.document_id;
    let preview = match fetch(
        core,
        &format!("v1/agent-approval-previews/operating-snapshots/{kind}/{id}"),
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
        &format!("v1/agent-approvals/operating-snapshots/{kind}/{id}"),
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
        let document = &result["createdDocument"];
        let existing = &preview["document"]["existingSnapshot"];
        if !uuid(&document["id"])
            || document["traceId"] != json!(context.trace_id)
            || result["status"] != "executed"
            || document["ownerUserId"] != preview["document"]["ownerUserId"]
            || document["sourceHash"] != preview["document"]["sourceHash"]
            || document["dataQualityStatus"] != preview["document"]["dataQualityStatus"]
            || document["utcOffsetMinutes"] != preview["document"]["input"]["utcOffsetMinutes"]
            || document["created"] != json!(existing.is_null())
            || !validation::timestamp(&document["generatedAt"])
            || (!existing.is_null()
                && (document["id"] != existing["id"]
                    || document["generatedAt"] != existing["generatedAt"]))
            || command.decision != business_core::document_approval::ApprovalDecision::Approve
        {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        let target = document["id"].as_str().and_then(|s| s.parse::<Uuid>().ok());
        if let Some(id) = target {
            result["resourceRefs"] = json!([resource(id)]);
        }
    }

    if result["executed"] == false
        && (!result["createdDocument"].is_null()
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
#[path = "operating_snapshot_writes_tests.rs"]
mod tests;
