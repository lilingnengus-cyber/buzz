//! Count reads use Core authority and intersect the current agent delegation.
use super::*;
use business_query_contracts::{
    GetBusinessDocumentInput, SearchInventoryCountOptionsInput, SearchInventoryCountsInput,
    ValidateInput,
};
use std::collections::BTreeSet;

pub(super) fn handles(tool: &str) -> bool {
    matches!(
        tool,
        "search_inventory_counts" | "get_inventory_count" | "search_inventory_count_options"
    )
}

fn normalized<T: serde::de::DeserializeOwned + Serialize + ValidateInput>(
    input: &Value,
) -> Option<Value> {
    let mut input: T = serde_json::from_value(input.clone()).ok()?;
    input
        .validate_and_normalize(chrono::Utc::now().date_naive())
        .ok()?;
    serde_json::to_value(input).ok()
}
fn dimension(item: &Value, key: &str, allowed: &BTreeSet<String>, nullable: bool) -> bool {
    match item.get(key) {
        Some(Value::String(id)) => {
            Uuid::parse_str(id).is_ok() && (allowed.is_empty() || allowed.contains(id))
        }
        Some(Value::Null) => nullable,
        _ => false,
    }
}
pub(super) fn permits(item: &Value, scope: &AuthorizationScope, options: bool) -> bool {
    if !dimension(item, "legalEntityId", &scope.legal_entity_ids, false)
        || !dimension(item, "warehouseId", &scope.warehouse_ids, false)
        || !dimension(item, "businessUnitId", &scope.business_unit_ids, false)
    {
        return false;
    }
    if options {
        return dimension(item, "brandId", &scope.brand_ids, true);
    }
    dimension(
        item,
        "snapshotBusinessUnitId",
        &scope.business_unit_ids,
        true,
    ) && item["lines"].as_array().is_some_and(|lines| {
        !lines.is_empty()
            && lines.iter().all(|line| {
                dimension(line, "brandId", &scope.brand_ids, true)
                    && dimension(line, "snapshotBrandId", &scope.brand_ids, true)
            })
    })
}

pub(super) async fn read(
    core: &CoreClient,
    tool: &str,
    input: &Value,
    scope: &AuthorizationScope,
    context: &RequestContext,
) -> Response {
    let options = tool == "search_inventory_count_options";
    let detail = tool == "get_inventory_count";
    let filter = match tool {
        "search_inventory_counts" => normalized::<SearchInventoryCountsInput>(input),
        "search_inventory_count_options" => normalized::<SearchInventoryCountOptionsInput>(input),
        "get_inventory_count" => normalized::<GetBusinessDocumentInput>(input),
        _ => None,
    };
    let Some(filter) = filter else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let path = if detail {
        format!(
            "v1/agent-inventory-counts/{}",
            filter["documentId"].as_str().unwrap_or_default()
        )
    } else if options {
        "v1/agent-inventory-count-options".into()
    } else {
        "v1/agent-inventory-counts".into()
    };
    let Ok(mut url) = core.base_url.join(&path) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if !detail {
        if let Some(fields) = filter.as_object() {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in fields {
                if let Some(value) = value.as_str() {
                    pairs.append_pair(key, value);
                } else if value.is_number() {
                    pairs.append_pair(key, &value.to_string());
                }
            }
        }
    }
    let response = core
        .client
        .get(url)
        .header("x-business-service-credential", &core.credential)
        .header("x-service-audience", "business-core")
        .header(
            "x-enterprise-user-id",
            context.enterprise_user_id.to_string(),
        )
        .header("x-trace-id", context.trace_id.to_string())
        .send()
        .await;
    let Ok(response) = response else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if matches!(response.status().as_u16(), 401 | 403 | 404) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !response.status().is_success() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let Ok(envelope) = response.json::<Value>().await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if envelope["traceId"].as_str() != Some(context.trace_id.to_string().as_str()) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let (mut items, has_more, next) = if detail {
        if !envelope["item"].is_object() || envelope["item"]["id"] != filter["documentId"] {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        (vec![envelope["item"].clone()], false, None)
    } else {
        let Some(items) = envelope["items"].as_array() else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let limit = filter["limit"].as_u64().unwrap_or(0) as usize;
        let offset = filter["offset"].as_u64().unwrap_or(0);
        let more = items.len() > limit;
        (
            items.iter().take(limit).cloned().collect::<Vec<_>>(),
            more,
            more.then_some(offset + limit as u64),
        )
    };
    items.retain(|item| permits(item, scope, options));
    if detail && items.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let refs = if options {
        vec![]
    } else {
        items
            .iter()
            .filter_map(|item| {
                let id = item["id"].as_str()?;
                Uuid::parse_str(id).ok()?;
                Some(ResourceRef {
                    r#type: "inventory_count".into(),
                    id: Some(id.into()),
                    title: "查看库存盘点".into(),
                    biz_uri: format!("biz://inventory-count/{id}"),
                })
            })
            .collect()
    };
    Json(BusinessToolResult {
        schema_version: 1,
        status: BusinessToolStatus::Ok,
        as_of: chrono::Utc::now(),
        scope_summary: ScopeSummary {
            legal_entity_ids: scope.legal_entity_ids.iter().cloned().collect(),
            ..Default::default()
        },
        summary: BTreeMap::from([
            ("source".into(), json!("business-core-inventory-counts")),
            ("nextOffset".into(), json!(next)),
            (
                "requiresDisambiguation".into(),
                json!(has_more || items.len() > 1),
            ),
        ]),
        items,
        resource_refs: refs,
        pagination: Some(Pagination {
            next_cursor: next.map(|n| n.to_string()),
            has_more,
        }),
        evidence: vec![],
        warnings: vec![],
        trace_id: context.trace_id,
    })
    .into_response()
}

#[cfg(test)]
#[path = "inventory_counts_tests.rs"]
mod tests;
