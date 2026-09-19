//! CRM reads intersect current Core authority with the agent delegation.
use super::*;
use business_query_contracts::{
    GetCrmOpportunityInput, SearchCrmOpportunitiesInput, ValidateInput,
};
use std::collections::BTreeSet;

pub(super) fn handles(tool: &str) -> bool {
    matches!(tool, "search_crm_opportunities" | "get_crm_opportunity")
}
fn normalized<T: serde::de::DeserializeOwned + Serialize + ValidateInput>(
    input: &Value,
) -> Option<Value> {
    let mut value: T = serde_json::from_value(input.clone()).ok()?;
    value
        .validate_and_normalize(chrono::Utc::now().date_naive())
        .ok()?;
    serde_json::to_value(value).ok()
}
fn dimension(item: &Value, key: &str, allowed: &BTreeSet<String>, nullable: bool) -> bool {
    match &item[key] {
        Value::String(id) => {
            Uuid::parse_str(id).is_ok() && (allowed.is_empty() || allowed.contains(id))
        }
        Value::Null => nullable && item.get(key).is_some(),
        _ => false,
    }
}
pub(super) fn permits(item: &Value, scope: &AuthorizationScope) -> bool {
    dimension(item, "legalEntityId", &scope.legal_entity_ids, false)
        && dimension(item, "businessUnitId", &scope.business_unit_ids, false)
        && dimension(item, "customerId", &scope.customer_ids, true)
}
pub(super) async fn read(
    core: &CoreClient,
    tool: &str,
    input: &Value,
    scope: &AuthorizationScope,
    context: &RequestContext,
) -> Response {
    let detail = tool == "get_crm_opportunity";
    let filter = if detail {
        normalized::<GetCrmOpportunityInput>(input)
    } else {
        normalized::<SearchCrmOpportunitiesInput>(input)
    };
    let Some(filter) = filter else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let path = if detail {
        "v1/agent-crm-opportunity"
    } else {
        "v1/agent-crm-opportunities"
    };
    let Ok(mut url) = core.base_url.join(path) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
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
    if response.status().as_u16() == 409 {
        return StatusCode::CONFLICT.into_response();
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
    let Some(has_more) = envelope["hasMore"].as_bool() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let offset = filter["offset"].as_u64().unwrap_or(0);
    let limit = filter["limit"].as_u64().unwrap_or(0);
    let next = has_more.then_some(offset + limit);
    if envelope["nextOffset"] != json!(next) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let mut items = if detail {
        let mut item = envelope["item"].clone();
        if !item.is_object() || item["id"] != filter["documentId"] {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        if !permits(&item, scope) {
            return StatusCode::NOT_FOUND.into_response();
        }
        if filter["expectedVersion"]
            .as_i64()
            .is_some_and(|v| item["version"].as_i64() != Some(v))
        {
            return StatusCode::CONFLICT.into_response();
        }
        let Some(notes) = envelope["followups"].as_array() else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        if notes.len() > limit as usize || (has_more && notes.len() != limit as usize) {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        item["followups"] = json!(notes);
        vec![item]
    } else {
        let Some(items) = envelope["items"].as_array() else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        if items.len() > limit as usize || (has_more && items.len() != limit as usize) {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        items.clone()
    };
    if items.iter().any(|item| {
        item["id"]
            .as_str()
            .is_none_or(|id| Uuid::parse_str(id).is_err())
            || item["version"].as_i64().is_none_or(|v| v < 1)
    }) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    items.retain(|item| permits(item, scope));
    let refs = items
        .iter()
        .filter_map(|item| {
            let id = item["id"].as_str()?;
            Some(ResourceRef {
                r#type: "crm_opportunity".into(),
                id: Some(id.into()),
                title: "查看商机".into(),
                biz_uri: format!("biz://crm-opportunity/{id}"),
            })
        })
        .collect();
    Json(BusinessToolResult {
        schema_version: 1,
        status: BusinessToolStatus::Ok,
        as_of: chrono::Utc::now(),
        scope_summary: ScopeSummary {
            legal_entity_ids: scope.legal_entity_ids.iter().cloned().collect(),
            ..Default::default()
        },
        summary: BTreeMap::from([
            ("source".into(), json!("business-core-crm")),
            ("nextOffset".into(), json!(next)),
            (
                "requiresDisambiguation".into(),
                json!(!detail && (has_more || items.len() > 1)),
            ),
            (
                "amountUnit".into(),
                json!("expectedAmountMinor is in minor currency units; CNY 100 equals 1 yuan"),
            ),
        ]),
        items,
        resource_refs: refs,
        pagination: Some(Pagination {
            next_cursor: next.map(|v| v.to_string()),
            has_more,
        }),
        evidence: vec![],
        warnings: vec![],
        trace_id: context.trace_id,
    })
    .into_response()
}

#[cfg(test)]
#[path = "crm_tests.rs"]
mod tests;
