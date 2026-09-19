use super::*;
use business_query_contracts::{SearchMasterDataInput, ValidateInput};

pub(super) async fn search(
    core: &CoreClient,
    input: &Value,
    scope: &AuthorizationScope,
    context: &RequestContext,
) -> Response {
    let Ok(mut input) = serde_json::from_value::<SearchMasterDataInput>(input.clone()) else {
        return (StatusCode::BAD_REQUEST, "invalid_filter").into_response();
    };
    if input
        .validate_and_normalize(chrono::Utc::now().date_naive())
        .is_err()
    {
        return (StatusCode::BAD_REQUEST, "invalid_filter").into_response();
    }
    let Ok(mut url) = core
        .base_url
        .join(&format!("v1/master-data/{}", input.resource_type.as_str()))
    else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("query", input.query.as_deref().unwrap_or(""))
            .append_pair("offset", &input.offset.to_string())
            .append_pair("limit", &(input.limit + 1).to_string());
        if let Some(id) = input.legal_entity_id {
            query.append_pair("legalEntityId", &id.to_string());
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
    let Ok(mut envelope) = response.json::<Value>().await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Some(rows) = envelope.get_mut("items").and_then(Value::as_array_mut) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let has_more = rows.len() > input.limit as usize;
    rows.truncate(input.limit as usize);
    let items = rows
        .iter()
        .filter(|item| permitted(item, scope))
        .cloned()
        .collect::<Vec<_>>();
    let next_offset = has_more.then_some(input.offset + input.limit);
    Json(BusinessToolResult {
        schema_version: 1,
        status: BusinessToolStatus::Ok,
        as_of: chrono::Utc::now(),
        scope_summary: ScopeSummary {
            legal_entity_ids: scope.legal_entity_ids.iter().cloned().collect(),
            ..Default::default()
        },
        summary: BTreeMap::from([
            ("source".into(), json!("business-core-master-data")),
            ("resourceType".into(), json!(input.resource_type)),
            ("nextOffset".into(), json!(next_offset)),
            (
                "requiresDisambiguation".into(),
                json!(has_more || items.len() > 1),
            ),
        ]),
        items,
        pagination: Some(Pagination {
            next_cursor: next_offset.map(|offset| offset.to_string()),
            has_more,
        }),
        resource_refs: vec![],
        evidence: vec![],
        warnings: vec![],
        trace_id: context.trace_id,
    })
    .into_response()
}

fn permitted(item: &Value, scope: &AuthorizationScope) -> bool {
    [
        ("legalEntityId", &scope.legal_entity_ids),
        ("warehouseId", &scope.warehouse_ids),
        ("customerId", &scope.customer_ids),
        ("supplierId", &scope.supplier_ids),
        ("brandId", &scope.brand_ids),
        ("businessUnitId", &scope.business_unit_ids),
    ]
    .iter()
    .all(|(key, allowed)| {
        allowed.is_empty()
            || item.get(key).is_none_or(Value::is_null)
            || item
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|id| allowed.contains(id))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn narrowed_turn_scope_filters_records() {
        let scope = AuthorizationScope {
            customer_ids: ["allowed".into()].into(),
            ..Default::default()
        };
        assert!(permitted(&json!({"customerId":"allowed"}), &scope));
        assert!(!permitted(&json!({"customerId":"other"}), &scope));
        assert!(permitted(
            &json!({"resourceType":"unit_of_measure","customerId":null}),
            &scope
        ));
    }

    #[tokio::test]
    async fn lookup_forwards_identity_and_preserves_source_pagination() {
        use axum::extract::Query;
        use std::collections::HashMap;
        let actor = Uuid::new_v4();
        let trace = Uuid::new_v4();
        let server = Router::new().route("/v1/master-data/customer", get(move |headers: HeaderMap, Query(query): Query<HashMap<String,String>>| async move {
            assert_eq!(headers["x-enterprise-user-id"], actor.to_string());
            assert_eq!(headers["x-trace-id"], trace.to_string());
            assert_eq!(headers["x-service-audience"], "business-core");
            assert_eq!(query["query"], "同名客户");
            assert_eq!(query["offset"], "5");
            assert_eq!(query["limit"], "2");
            Json(json!({"items":[{"id":"one","customerId":"outside"},{"id":"two","customerId":"allowed"}]}))
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, server).await.unwrap();
        });
        let core = CoreClient {
            client: reqwest::Client::new(),
            base_url: Url::parse(&format!("http://{address}/")).unwrap(),
            credential: "test-credential".into(),
        };
        let context = RequestContext {
            enterprise_user_id: actor,
            identity_binding_id: Uuid::new_v4(),
            delegation_id: Uuid::new_v4(),
            agent_id: "agent".into(),
            agent_turn_id: "turn".into(),
            trace_id: trace,
            used_calls: 1,
            required_scope: "business_master_data:read".into(),
            source_buzz_event_id: "a".repeat(64),
            source_channel_id: "channel".into(),
        };
        let scope = AuthorizationScope {
            customer_ids: ["allowed".into()].into(),
            ..Default::default()
        };
        let result = search(
            &core,
            &json!({"resourceType":"customer","query":"同名客户","offset":5,"limit":1}),
            &scope,
            &context,
        )
        .await;
        assert_eq!(result.status(), StatusCode::OK);
        let body = axum::body::to_bytes(result.into_body(), 65536)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["items"], json!([]));
        assert_eq!(body["pagination"]["hasMore"], true);
        assert_eq!(body["summary"]["nextOffset"], 6);
        assert_eq!(body["summary"]["requiresDisambiguation"], true);
        assert_eq!(body["traceId"], trace.to_string());
        task.abort();
    }
}
