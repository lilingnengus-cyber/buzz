//! Bounded views of complete, server-verified inventory approval snapshots.
use super::*;
use business_query_contracts::{GetInventoryCountPreviewInput, ValidateInput};
use inventory_count_writes::{bound_preview, count_ref, fetch, permits_snapshot};

pub(super) fn page(mut envelope: Value, offset: usize, limit: usize) -> Option<Value> {
    let document = &mut envelope["document"];
    let all = document["lines"].as_array()?;
    let total = all.len();
    let selected = all
        .iter()
        .skip(offset)
        .take(limit)
        .cloned()
        .collect::<Vec<_>>();
    // Keep command quantities and SKU selection aligned with the visible rows.
    if let Some(ids) = document
        .get_mut("command")
        .and_then(|v| v.get_mut("skuIds"))
        .and_then(Value::as_array_mut)
    {
        ids.retain(|id| selected.iter().any(|line| line["skuId"] == *id));
    }
    if let Some(lines) = document
        .get_mut("operation")
        .and_then(|v| v.get_mut("command"))
        .and_then(|v| v.get_mut("lines"))
        .and_then(Value::as_array_mut)
    {
        lines.retain(|input| {
            selected
                .iter()
                .any(|line| line["id"] == input["countLineId"])
        });
    }
    document["lines"] = json!(selected);
    let next = offset.saturating_add(limit);
    envelope["previewPagination"] = json!({"offset":offset,"limit":limit,"totalLines":total,"hasMore":next<total,"nextOffset":(next<total).then_some(next)});
    envelope["previewHashScope"] = json!("complete_server_snapshot");
    envelope["previewReadTool"] = json!("get_inventory_count_approval_preview");
    if let Some(item) = envelope["item"].as_object_mut() {
        item.remove("snapshot");
    }
    Some(envelope)
}

pub(super) async fn read(
    core: &CoreClient,
    input: &Value,
    scope: &AuthorizationScope,
    context: &RequestContext,
) -> Response {
    let Ok(mut input) = serde_json::from_value::<GetInventoryCountPreviewInput>(input.clone())
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if input
        .validate_and_normalize(chrono::Utc::now().date_naive())
        .is_err()
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let creation = input.document_type == "inventory_count_creation_intent";
    let category = if creation {
        "inventory-count-creations"
    } else {
        "inventory-count-operations"
    };
    let envelope = match fetch(
        core,
        &format!(
            "v1/agent-approval-previews/{category}/{}/{}",
            input.document_type, input.document_id
        ),
        None,
        context,
        "get_inventory_count_approval_preview",
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !bound_preview(&envelope, &input.document_type, context.trace_id)
        || envelope["item"]["id"] != json!(input.document_id)
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    // The whole snapshot must be authorized, including rows outside this page.
    if !permits_snapshot(&envelope["document"], scope, creation) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if envelope["previewHash"] != input.preview_hash {
        return StatusCode::CONFLICT.into_response();
    }
    let refs = if creation {
        vec![]
    } else {
        let Some(id) = envelope["document"]["source"]["id"]
            .as_str()
            .and_then(|v| v.parse::<Uuid>().ok())
        else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        vec![count_ref(id)]
    };
    let Some(mut preview) = page(envelope, input.offset as usize, input.limit as usize) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    preview["documentType"] = json!(input.document_type);
    let next = preview["previewPagination"]["nextOffset"].clone();
    let has_more = preview["previewPagination"]["hasMore"].clone();
    Json(json!({"schemaVersion":1,"status":"ok","asOf":chrono::Utc::now(),"traceId":context.trace_id,"scopeSummary":{},"summary":{"nextOffset":next,"previewHashScope":"complete_server_snapshot"},"items":[preview],"resourceRefs":refs,"pagination":{"hasMore":has_more,"nextCursor":next.as_u64().map(|v|v.to_string())},"warnings":[],"evidence":[]})).into_response()
}
