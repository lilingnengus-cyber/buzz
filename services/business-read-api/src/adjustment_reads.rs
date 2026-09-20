//! Exact, bounded adjustment reads with independent delegated-scope checks.
use super::*;
use business_query_contracts::{
    GetOperationalAdjustmentInput, SearchOperationalAdjustmentsInput, ValidateInput,
};
mod validation;
use validation::*;
pub(super) fn handles(tool: &str) -> bool {
    matches!(
        tool,
        "search_operational_adjustments" | "get_operational_adjustment"
    )
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
pub(super) async fn read(
    core: &CoreClient,
    tool: &str,
    input: &Value,
    scope: &AuthorizationScope,
    context: &RequestContext,
) -> Response {
    let detail = tool == "get_operational_adjustment";
    let filter = if detail {
        normalized::<GetOperationalAdjustmentInput>(input)
    } else {
        normalized::<SearchOperationalAdjustmentsInput>(input)
    };
    let Some(filter) = filter else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let path = if detail {
        format!(
            "v1/profit-adjustments/{}",
            filter["documentId"].as_str().unwrap_or_default()
        )
    } else {
        "v1/profit-adjustments".into()
    };
    let Ok(mut url) = core.base_url.join(&path) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if let Some(fields) = filter.as_object() {
        let mut query = url.query_pairs_mut();
        for (key, value) in fields {
            if key == "documentId" {
                continue;
            }
            if let Some(value) = value.as_str() {
                query.append_pair(key, value);
            } else if value.is_number() {
                query.append_pair(key, &value.to_string());
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
    if matches!(response.status().as_u16(), 400 | 409 | 503) {
        return response.status().into_response();
    }
    if !response.status().is_success() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let Ok(v) = response.json::<Value>().await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if v["schemaVersion"] != 1
        || v["traceId"] != json!(context.trace_id)
        || v["boundary"] != "management_only_not_general_ledger"
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if !scope_covers(&v["scope"], scope) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some((items, next, has_more)) = convert(&v, &filter, detail) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if items.iter().any(|item| !attributed(item, scope)) {
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
        summary: BTreeMap::from([
            ("source".into(), json!("business-core-adjustments")),
            (
                "boundary".into(),
                json!("management_only_not_general_ledger"),
            ),
            (
                "requiresDisambiguation".into(),
                json!(!detail && (has_more || items.len() > 1)),
            ),
            ("detailLinkAvailable".into(), json!(true)),
        ]),
        resource_refs: items
            .iter()
            .filter_map(|item| {
                let id = item["id"].as_str()?;
                Some(ResourceRef {
                    r#type: "profit_adjustment".into(),
                    id: Some(id.into()),
                    title: item["adjustmentNumber"].as_str()?.into(),
                    biz_uri: format!("biz://profit-adjustment/{id}"),
                })
            })
            .collect(),
        items,
        pagination: Some(Pagination {
            next_cursor: next,
            has_more,
        }),
        evidence: vec![],
        warnings: vec![],
        trace_id: context.trace_id,
    })
    .into_response()
}
#[cfg(test)]
mod tests;
