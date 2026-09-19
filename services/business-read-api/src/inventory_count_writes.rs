//! Fixed count preparations and independently verified signed approvals.
use super::*;
use business_core::b2::{model::VersionCommand, CreateInventoryCount, SubmitInventoryCount};
use sha2::{Digest, Sha256};

pub(super) fn family(tool: &str) -> Option<(&'static str, &'static str)> {
    match tool {
        "prepare_inventory_count_creation" | "approve_inventory_count_creation" => Some((
            "inventory_count_creation_intent",
            "inventory-count-creation",
        )),
        "prepare_inventory_count_submission" | "approve_inventory_count_submission" => Some((
            "inventory_count_submission_intent",
            "inventory-count-operation",
        )),
        "prepare_inventory_count_posting" | "approve_inventory_count_posting" => Some((
            "inventory_count_posting_intent",
            "inventory-count-operation",
        )),
        "prepare_inventory_count_cancellation" | "approve_inventory_count_cancellation" => Some((
            "inventory_count_cancellation_intent",
            "inventory-count-operation",
        )),
        _ => None,
    }
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OperationInput<T> {
    inventory_count_id: Uuid,
    command: T,
}

fn canonical(tool: &str, input: &Value) -> Option<Value> {
    if tool == "prepare_inventory_count_creation" {
        let v: CreateInventoryCount = serde_json::from_value(input.clone()).ok()?;
        if v.sku_ids.is_empty()
            || v.sku_ids.len() > 500
            || v.sku_ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != v.sku_ids.len()
        {
            return None;
        }
        return serde_json::to_value(v).ok();
    }
    if tool == "prepare_inventory_count_submission" {
        let v: OperationInput<SubmitInventoryCount> = serde_json::from_value(input.clone()).ok()?;
        if v.command.expected_version <= 0
            || v.command.lines.is_empty()
            || v.command.lines.len() > 500
            || v.command
                .lines
                .iter()
                .map(|l| l.count_line_id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != v.command.lines.len()
            || v.command.lines.iter().any(|l| {
                l.actual_on_hand_quantity.0 < Decimal::ZERO
                    || l.surplus_unit_cost.is_some_and(|v| v.0 < Decimal::ZERO)
            })
        {
            return None;
        }
        return Some(
            json!({"inventoryCountId":v.inventory_count_id,"operation":{"operation":"submit","command":v.command}}),
        );
    }
    let v: OperationInput<VersionCommand> = serde_json::from_value(input.clone()).ok()?;
    if v.command.expected_version <= 0 {
        return None;
    }
    let operation = match tool {
        "prepare_inventory_count_posting" => "post",
        "prepare_inventory_count_cancellation"
            if v.command
                .reason_code
                .as_ref()
                .is_some_and(|r| !r.trim().is_empty() && r.chars().count() <= 1000) =>
        {
            "cancel"
        }
        _ => return None,
    };
    Some(
        json!({"inventoryCountId":v.inventory_count_id,"operation":{"operation":operation,"command":v.command}}),
    )
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
pub(super) fn permits_snapshot(
    snapshot: &Value,
    scope: &AuthorizationScope,
    creation: bool,
) -> bool {
    let mut document = if creation {
        snapshot["command"].clone()
    } else {
        snapshot["source"].clone()
    };
    if !document.is_object() {
        return false;
    }
    if creation {
        document["businessUnitId"] = snapshot["businessUnitId"].clone();
        document["snapshotBusinessUnitId"] = Value::Null;
    }
    document["lines"] = snapshot["lines"].clone();
    if creation {
        if let Some(lines) = document["lines"].as_array_mut() {
            for line in lines {
                if !line.is_object() {
                    return false;
                }
                line["snapshotBrandId"] = Value::Null;
            }
        }
    }
    inventory_counts::permits(&document, scope, false)
}
pub(super) fn bound_preview(value: &Value, kind: &str, trace: Uuid) -> bool {
    let Some(id) = value["item"]["id"]
        .as_str()
        .and_then(|v| v.parse::<Uuid>().ok())
    else {
        return false;
    };
    let Ok(bytes) = serde_json::to_vec(&value["document"]) else {
        return false;
    };
    let hash: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    value["traceId"] == json!(trace)
        && value["item"]["version"] == 1
        && value["item"]["snapshot"] == value["document"]
        && value["previewHash"] == hash
        && value["approvalCommand"] == format!("确认 {} {id} v1 {hash}", kind.replace('_', "-"))
        && value["rejectionCommand"] == format!("拒绝 {} {id} v1 {hash}", kind.replace('_', "-"))
}
pub(super) async fn fetch(
    core: &CoreClient,
    path: &str,
    input: Option<&Value>,
    context: &RequestContext,
    tool: &str,
) -> Result<Value, Response> {
    let url = core
        .base_url
        .join(path)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE.into_response())?;
    let request = if let Some(input) = input {
        core.client.post(url).json(input)
    } else {
        core.client.get(url)
    };
    let response = request
        .header("x-business-service-credential", &core.credential)
        .header("x-service-audience", "business-core")
        .header(
            "x-enterprise-user-id",
            context.enterprise_user_id.to_string(),
        )
        .header("x-trace-id", context.trace_id.to_string())
        .header(
            "idempotency-key",
            format!("agent:{}:{tool}", context.delegation_id),
        )
        .send()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE.into_response())?;
    let status = response.status();
    let value = response
        .json::<Value>()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE.into_response())?;
    if !status.is_success() {
        return Err((status, Json(value)).into_response());
    }
    if value["traceId"] != json!(context.trace_id) {
        return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
    }
    Ok(value)
}
pub(super) fn count_ref(id: Uuid) -> Value {
    json!({"type":"inventory_count","id":id,"title":"查看库存盘点","bizUri":format!("biz://inventory-count/{id}")})
}
pub(super) async fn forward(
    core: &CoreClient,
    tool: &str,
    input: Value,
    context: &RequestContext,
    grant: &EffectiveGrant,
) -> Response {
    let Some((kind, category)) = family(tool) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(scope) = iam_authorization_scope(grant, &context.required_scope) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let creation = kind == "inventory_count_creation_intent";
    if tool.starts_with("prepare_") {
        let Some(input) = canonical(tool, &input) else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let preview = match fetch(
            core,
            &format!("v1/agent-{category}-previews/{kind}"),
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
        let matches = if creation {
            snapshot["command"] == input
        } else {
            snapshot["source"]["id"] == input["inventoryCountId"]
                && snapshot["operation"] == input["operation"]
                && snapshot["source"]["version"] == input["operation"]["command"]["expectedVersion"]
        };
        if !matches {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        if !permits_snapshot(snapshot, &scope, creation) {
            return StatusCode::FORBIDDEN.into_response();
        }
        let mut prepared = match fetch(
            core,
            &format!("v1/agent-{category}-intents/{kind}"),
            Some(&input),
            context,
            tool,
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
        // The complete hash-bound document is already returned once below.
        // Keep the Core snapshot for verification, not a duplicate model payload.
        if let Some(item) = prepared["item"].as_object_mut() {
            item.remove("snapshot");
        }
        prepared["item"]["status"] = json!("draft");
        prepared["schemaVersion"] = json!(1);
        prepared["status"] = json!("ok");
        prepared["documentType"] = json!(kind);
        prepared["resourceRefs"] = if creation {
            json!([])
        } else {
            let Some(id) = input["inventoryCountId"]
                .as_str()
                .and_then(|v| v.parse::<Uuid>().ok())
            else {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            };
            json!([count_ref(id)])
        };
        return match super::inventory_count_previews::page(prepared, 0, 20) {
            Some(prepared) => Json(prepared).into_response(),
            None => StatusCode::SERVICE_UNAVAILABLE.into_response(),
        };
    }
    let Ok(command) = serde_json::from_value::<Approval>(input) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let id = command.document_id;
    let category = if creation {
        "inventory-count-creations"
    } else {
        "inventory-count-operations"
    };
    let preview = match fetch(
        core,
        &format!("v1/agent-approval-previews/{category}/{kind}/{id}"),
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
    if preview["previewHash"] != command.preview_hash || command.expected_version != 1 {
        return StatusCode::CONFLICT.into_response();
    }
    if !permits_snapshot(&preview["document"], &scope, creation) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let input = json!({"expectedVersion":command.expected_version,"previewHash":command.preview_hash,"decision":command.decision,"sourceBuzzEventId":context.source_buzz_event_id,"sourceChannelId":context.source_channel_id});
    let mut result = match fetch(
        core,
        &format!("v1/agent-approvals/{category}/{kind}/{id}"),
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
        let document = &result[if creation {
            "createdDocument"
        } else {
            "updatedDocument"
        }];
        let Some(count_id) = document["id"].as_str().and_then(|v| v.parse::<Uuid>().ok()) else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let expected_status = match kind {
            "inventory_count_creation_intent" => "counting",
            "inventory_count_submission_intent" => "counted",
            "inventory_count_posting_intent" => "posted",
            _ => "cancelled",
        };
        let expected_version = if creation {
            Some(1)
        } else {
            preview["document"]["source"]["version"]
                .as_i64()
                .and_then(|version| version.checked_add(1))
        };
        if result["status"] != "executed"
            || document["status"] != expected_status
            || document["version"].as_i64() != expected_version
            || expected_version.is_none()
            || document["traceId"] != json!(context.trace_id)
        {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        if !creation && json!(count_id) != preview["document"]["source"]["id"] {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        result["resourceRefs"] = json!([count_ref(count_id)]);
    }
    Json(result).into_response()
}

#[cfg(test)]
#[path = "inventory_count_writes_tests.rs"]
mod tests;
