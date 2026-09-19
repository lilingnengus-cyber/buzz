//! Fixed master commands, preserving omitted fields and intersecting delegation scope.
use super::*;
use inventory_count_writes::{bound_preview, fetch};
mod input;
mod scope;
use input::{Approval, Patch};

pub(super) fn family(tool: &str) -> Option<(&'static str, &'static str)> {
    match tool {
        "prepare_core_master_creation" | "approve_core_master_creation" => {
            Some(("core_master_creation_intent", "core"))
        }
        "prepare_core_master_update" | "approve_core_master_update" => {
            Some(("core_master_update_intent", "core"))
        }
        "prepare_product_master_creation" | "approve_product_master_creation" => {
            Some(("product_master_creation_intent", "product"))
        }
        "prepare_product_master_update" | "approve_product_master_update" => {
            Some(("product_master_update_intent", "product"))
        }
        _ => None,
    }
}
pub(super) fn valid(tool: &str, value: &Value) -> bool {
    let Some((_, family)) = family(tool) else {
        return false;
    };
    if tool.starts_with("approve_") {
        serde_json::from_value::<Approval>(value.clone()).is_ok_and(|v| {
            v.expected_version == 1
                && v.preview_hash.len() == 64
                && v.preview_hash.bytes().all(|c| c.is_ascii_hexdigit())
        })
    } else if tool.ends_with("_creation") {
        input::canonical(family, value.clone(), None).is_some()
    } else {
        serde_json::from_value::<Patch>(value.clone()).is_ok_and(|v| v.valid(family))
    }
}
pub(super) async fn forward(
    core: &CoreClient,
    tool: &str,
    value: Value,
    context: &RequestContext,
    grant: &EffectiveGrant,
) -> Response {
    let Some((kind, family)) = family(tool) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(scope) = authorization_scope(grant, &context.required_scope) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if !valid(tool, &value) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if tool.starts_with("prepare_") {
        let command = if tool.ends_with("_creation") {
            input::canonical(family, value, None)
        } else {
            let Ok(patch) = serde_json::from_value::<Patch>(value) else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            let record = match fetch(
                core,
                &format!(
                    "v1/agent-{family}-master-records/{}/{}",
                    patch.resource_type, patch.document_id
                ),
                None,
                context,
                tool,
            )
            .await
            {
                Ok(v) => v,
                Err(e) => return e,
            };
            if record["item"]["id"] != json!(patch.document_id)
                || record["item"]["resourceType"] != patch.resource_type
            {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
            if !scope::record(&record["item"], &scope) {
                return StatusCode::FORBIDDEN.into_response();
            }
            patch.merge(&record["item"])
        };
        let Some(command) = command else {
            return StatusCode::CONFLICT.into_response();
        };
        let preview = match fetch(
            core,
            &format!("v1/agent-{family}-master-previews"),
            Some(&command),
            context,
            tool,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => return e,
        };
        let snapshot = &preview["document"];
        if snapshot["command"] != command {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        if !scope::preview(snapshot, &scope, kind) {
            return StatusCode::FORBIDDEN.into_response();
        }
        let mut prepared = match fetch(
            core,
            &format!("v1/agent-master-intents/{kind}"),
            Some(&command),
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
        // No synthetic master URI: the client detail route will be added with MCP exposure.
        prepared["resourceRefs"] = json!([]);
        return Json(prepared).into_response();
    }
    let Ok(command) = serde_json::from_value::<Approval>(value) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let id = command.document_id;
    let preview = match fetch(
        core,
        &format!("v1/agent-approval-previews/master/{kind}/{id}"),
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
    if preview["previewHash"] != command.preview_hash {
        return StatusCode::CONFLICT.into_response();
    }
    if !scope::preview(&preview["document"], &scope, kind) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let input = json!({"expectedVersion":1,"previewHash":command.preview_hash,"decision":command.decision,"sourceBuzzEventId":context.source_buzz_event_id,"sourceChannelId":context.source_channel_id});
    let mut result = match fetch(
        core,
        &format!("v1/agent-approvals/master/{kind}/{id}"),
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
    if result["executed"] == true {
        let document = &result["createdDocument"];
        let expected = if kind.ends_with("creation_intent") {
            Some(1)
        } else {
            preview["document"]["current"]["version"]
                .as_i64()
                .and_then(|v| v.checked_add(1))
        };
        let Some(target) = document["id"].as_str().and_then(|v| v.parse::<Uuid>().ok()) else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let creation = kind.ends_with("creation_intent");
        let expected_status = if creation {
            json!("active")
        } else {
            preview["document"]["current"]["status"].clone()
        };
        let expected_code = if creation {
            &preview["document"]["command"]["command"]["code"]
        } else {
            &preview["document"]["current"]["code"]
        };
        if expected.is_none()
            || document["version"].as_i64() != expected
            || document["traceId"] != json!(context.trace_id)
            || document["resourceType"] != preview["document"]["resourceType"]
            || result["status"] != "executed"
            || document["status"] != expected_status
            || ((!creation || document["resourceType"] != "uom_conversion")
                && document["code"] != *expected_code)
            || (kind.ends_with("update_intent")
                && json!(target) != preview["document"]["documentId"])
        {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }
    result["resourceRefs"] = if result["executed"] == true {
        json!([resource_ref(&result["createdDocument"])])
    } else {
        json!([])
    };
    Json(result).into_response()
}
#[cfg(test)]
#[path = "master_writes/tests.rs"]
mod tests;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecordInput {
    resource_type: String,
    document_id: Uuid,
}

pub(super) async fn read(
    core: &CoreClient,
    input: &Value,
    scope: &AuthorizationScope,
    context: &RequestContext,
) -> Response {
    let Ok(input) = serde_json::from_value::<RecordInput>(input.clone()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(family) = input::family_of_resource(&input.resource_type) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let record = match fetch(
        core,
        &format!(
            "v1/agent-{family}-master-records/{}/{}",
            input.resource_type, input.document_id
        ),
        None,
        context,
        "get_business_master_record",
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return e,
    };
    let item = &record["item"];
    if item["id"] != json!(input.document_id)
        || item["resourceType"] != input.resource_type
        || item["version"].as_i64().is_none_or(|v| v <= 0)
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if !scope::record(item, scope) {
        return StatusCode::FORBIDDEN.into_response();
    }
    Json(BusinessToolResult {
        schema_version: 1,
        status: BusinessToolStatus::Ok,
        as_of: chrono::Utc::now(),
        scope_summary: ScopeSummary {
            legal_entity_ids: scope.legal_entity_ids.iter().cloned().collect(),
            ..Default::default()
        },
        summary: BTreeMap::from([("source".into(), json!("business-core-master-record"))]),
        items: vec![item.clone()],
        pagination: None,
        resource_refs: vec![resource_ref(item)],
        evidence: vec![],
        warnings: vec![],
        trace_id: context.trace_id,
    })
    .into_response()
}

pub(super) fn authorization_scope(
    grant: &EffectiveGrant,
    required: &str,
) -> Option<AuthorizationScope> {
    scope::delegation(grant, required)
}

fn resource_ref(item: &Value) -> business_query_contracts::ResourceRef {
    let kind = item["resourceType"].as_str().unwrap_or_default();
    let id = item["id"].as_str().unwrap_or_default();
    business_query_contracts::ResourceRef {
        r#type: "master_data".into(),
        id: Some(id.into()),
        title: item["code"].as_str().unwrap_or("基础资料").into(),
        biz_uri: format!("biz://master-data/{kind}/{id}"),
    }
}
