//! Fixed order-hold preparations and independently verified signed approvals.
use super::*;
use inventory_count_writes::{bound_preview, fetch, fetch_bound};
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
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    source_document_id: Uuid,
    expected_source_version: i64,
    reason: String,
}
fn canonical(tool: &str, input: &Value) -> Option<Value> {
    if !tool.starts_with("prepare_") || family(tool).is_none() {
        return None;
    }
    let mut v: Input = serde_json::from_value(input.clone()).ok()?;
    v.reason = v.reason.trim().to_owned();
    if v.expected_source_version < 1 || v.reason.is_empty() || v.reason.len() > 64 {
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
fn valid_snapshot(v: &Value, kind: &str) -> bool {
    let place = kind == "sales_order_hold_intent";
    if !matches!(
        kind,
        "sales_order_hold_intent" | "sales_order_release_hold_intent"
    ) {
        return false;
    }
    let source = &v["source"];
    let Some(version) = source["version"].as_i64().filter(|v| *v > 0) else {
        return false;
    };
    let Some(expected) = v["expectedVersion"].as_i64().filter(|v| *v > 0) else {
        return false;
    };
    let Some(reason) = v["reasonCode"].as_str() else {
        return false;
    };
    let Some(lines) = v["lines"].as_array().filter(|v| !v.is_empty()) else {
        return false;
    };
    ["id", "legalEntityId", "customerId", "businessUnitId"]
        .iter()
        .all(|k| uuid(&source[*k]))
        && (source["brandId"].is_null() || uuid(&source["brandId"]))
        && lines.iter().all(|l| {
            ["id", "skuId", "warehouseId", "businessUnitId"]
                .iter()
                .all(|k| uuid(&l[*k]))
                && (l["brandId"].is_null() || uuid(&l["brandId"]))
        })
        && !reason.trim().is_empty()
        && reason.len() <= 64
        && v["operation"] == if place { "place_hold" } else { "release_hold" }
        && v["targetHoldStatus"] == if place { "manual_review_hold" } else { "none" }
        && v["blocksShipmentCreationAndConfirmation"] == place
        && v["changesInventoryReservation"] == false
        && v["canExecute"]
            == json!(
                source["lifecycleStatus"] == "confirmed"
                    && source["holdStatus"] == if place { "none" } else { "manual_review_hold" }
                    && version == expected
            )
}
fn permits(v: &Value, scope: &AuthorizationScope, kind: &str) -> bool {
    valid_snapshot(v, kind) && permits_document(v, scope)
}
fn binds(v: &Value, input: &Value) -> bool {
    v["source"]["id"] == input["sourceDocumentId"]
        && v["expectedVersion"] == input["expectedSourceVersion"]
        && v["reasonCode"] == input["reason"]
}
fn resource(id: Uuid) -> Value {
    json!({"type":"sales_order","id":id,"title":"查看销售订单","bizUri":format!("biz://sales-order/{id}")})
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
            &format!("v1/agent-order-hold-previews/{kind}"),
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
        if !valid_snapshot(snapshot, kind) || !binds(snapshot, &input) {
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
            &format!("v1/agent-order-hold-intents/{kind}"),
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
        let id: Uuid = match snapshot["source"]["id"]
            .as_str()
            .and_then(|s| s.parse().ok())
        {
            Some(id) => id,
            None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        };
        prepared["resourceRefs"] = json!([resource(id)]);
        return Json(prepared).into_response();
    }
    let Ok(command) = serde_json::from_value::<Approval>(input) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let id = command.document_id;
    let preview = match fetch(
        core,
        &format!("v1/agent-approval-previews/order-holds/{kind}/{id}"),
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
        &format!("v1/agent-approvals/order-holds/{kind}/{id}"),
        Some(&input),
        context,
        tool,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return e,
    };
    if result["documentId"] != json!(id)
        || result["documentType"] != kind
        || !result["executed"].is_boolean()
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    result["resourceRefs"] = json!([]);
    if result["executed"] == true {
        let document = &result["createdDocument"];
        let Some(target) = document["id"].as_str().and_then(|v| v.parse::<Uuid>().ok()) else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let expected = preview["document"]["source"]["version"]
            .as_i64()
            .and_then(|v| v.checked_add(1));
        if expected.is_none()
            || document["version"].as_i64() != expected
            || document["traceId"] != json!(context.trace_id)
            || result["status"] != "executed"
            || json!(target) != preview["document"]["source"]["id"]
            || document["status"] != preview["document"]["targetHoldStatus"]
            || command.decision != business_core::document_approval::ApprovalDecision::Approve
        {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        result["resourceRefs"] = json!([resource(target)]);
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
#[path = "order_hold_writes_tests.rs"]
mod tests;
