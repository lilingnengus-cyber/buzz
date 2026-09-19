//! Fixed CRM preparations and independently verified signed approvals.
use super::*;
use business_core::crm::{AddFollowup, CrmCommand, SaveOpportunity};
use inventory_count_writes::{bound_preview, fetch};

pub(super) fn family(tool: &str) -> Option<&'static str> {
    match tool {
        "prepare_crm_creation" | "approve_crm_creation" => Some("crm_creation_intent"),
        "prepare_crm_update" | "approve_crm_update" => Some("crm_update_intent"),
        "prepare_crm_followup" | "approve_crm_followup" => Some("crm_followup_intent"),
        _ => None,
    }
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Target<T> {
    opportunity_id: Uuid,
    command: T,
}
fn canonical(tool: &str, input: &Value) -> Option<Value> {
    let command = match tool {
        "prepare_crm_creation" => {
            let command: SaveOpportunity = serde_json::from_value(input.clone()).ok()?;
            if command.expected_version.is_some() {
                return None;
            }
            CrmCommand::Create { command }
        }
        "prepare_crm_update" => {
            let v: Target<SaveOpportunity> = serde_json::from_value(input.clone()).ok()?;
            if v.command.expected_version.is_none_or(|v| v < 1) {
                return None;
            }
            CrmCommand::Update {
                opportunity_id: v.opportunity_id,
                command: v.command,
            }
        }
        "prepare_crm_followup" => {
            let v: Target<AddFollowup> = serde_json::from_value(input.clone()).ok()?;
            if v.command.expected_version < 1 {
                return None;
            }
            CrmCommand::Followup {
                opportunity_id: v.opportunity_id,
                command: v.command,
            }
        }
        _ => return None,
    };
    serde_json::to_value(command).ok()
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
fn permits(snapshot: &Value, scope: &AuthorizationScope, kind: &str) -> bool {
    if snapshot["documentType"] != kind || !crm::permits(snapshot, scope) {
        return false;
    }
    if kind != "crm_followup_intent" {
        let command = &snapshot["command"]["command"];
        if !crm::permits(command, scope)
            || ["legalEntityId", "businessUnitId", "customerId"]
                .iter()
                .any(|key| snapshot[*key] != command[*key])
        {
            return false;
        }
    } else if ["legalEntityId", "businessUnitId", "customerId"]
        .iter()
        .any(|key| snapshot[*key] != snapshot["current"][*key])
    {
        return false;
    }
    if kind == "crm_creation_intent" {
        snapshot["current"].is_null() && snapshot["opportunityId"].is_null()
    } else {
        crm::permits(&snapshot["current"], scope)
            && snapshot["current"]["id"] == snapshot["opportunityId"]
            && snapshot["current"]["id"] == snapshot["command"]["opportunityId"]
            && snapshot["current"]["version"] == snapshot["command"]["command"]["expectedVersion"]
    }
}
fn resource(id: Uuid) -> Value {
    json!({"type":"crm_opportunity","id":id,"title":"查看商机","bizUri":format!("biz://crm-opportunity/{id}")})
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
            &format!("v1/agent-crm-previews/{kind}"),
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
        if snapshot["command"] != input {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        if !permits(snapshot, &scope, kind) {
            return StatusCode::FORBIDDEN.into_response();
        }
        let mut prepared = match fetch(
            core,
            &format!("v1/agent-crm-intents/{kind}"),
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
        if let Some(item) = prepared["item"].as_object_mut() {
            item.remove("snapshot");
        }
        prepared["item"]["status"] = json!("draft");
        prepared["schemaVersion"] = json!(1);
        prepared["status"] = json!("ok");
        prepared["documentType"] = json!(kind);
        prepared["resourceRefs"] = if kind == "crm_creation_intent" {
            json!([])
        } else {
            let Some(id) = snapshot["opportunityId"]
                .as_str()
                .and_then(|v| v.parse::<Uuid>().ok())
            else {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            };
            json!([resource(id)])
        };
        return Json(prepared).into_response();
    }
    let Ok(command) = serde_json::from_value::<Approval>(input) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let id = command.document_id;
    let preview = match fetch(
        core,
        &format!("v1/agent-approval-previews/crm/{kind}/{id}"),
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
        &format!("v1/agent-approvals/crm/{kind}/{id}"),
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
        let expected = if kind == "crm_creation_intent" {
            Some(1)
        } else {
            preview["document"]["current"]["version"]
                .as_i64()
                .and_then(|v| v.checked_add(1))
        };
        if expected.is_none()
            || document["version"].as_i64() != expected
            || document["traceId"] != json!(context.trace_id)
            || result["status"] != "executed"
            || (kind != "crm_creation_intent"
                && json!(target) != preview["document"]["opportunityId"])
        {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        result["resourceRefs"] = json!([resource(target)]);
    }
    Json(result).into_response()
}

#[cfg(test)]
#[path = "crm_writes_tests.rs"]
mod tests;
