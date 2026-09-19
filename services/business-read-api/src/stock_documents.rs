use super::*;
use business_query_contracts::{SearchStockDocumentsInput, ValidateInput};

pub(super) async fn search(
    core: &CoreClient,
    kind: &str,
    input: &Value,
    scope: &AuthorizationScope,
    context: &RequestContext,
) -> Response {
    let Ok(mut input) = serde_json::from_value::<SearchStockDocumentsInput>(input.clone()) else {
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
        .join(&format!("v1/agent-stock-documents/{kind}"))
    else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("offset", &input.offset.to_string())
            .append_pair("limit", &input.limit.to_string());
        if let Some(text) = &input.query {
            query.append_pair("query", text);
        }
        for (key, value) in [
            ("documentId", input.document_id),
            ("partyId", input.party_id),
        ] {
            if let Some(id) = value {
                query.append_pair(key, &id.to_string());
            }
        }
        if let Some(status) = &input.status {
            query.append_pair("status", status);
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
        .filter(|item| permits_document(item, scope))
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
            ("source".into(), json!("business-core-stock-documents")),
            ("resourceType".into(), json!(kind)),
            ("nextOffset".into(), json!(next_offset)),
            (
                "requiresDisambiguation".into(),
                json!(has_more || items.len() > 1),
            ),
        ]),
        resource_refs: items
            .iter()
            .filter_map(|item| {
                let (resource, id) = match kind {
                    "shipment" => ("shipment", item["id"].as_str()?),
                    "goods_receipt" => ("goods-receipt", item["id"].as_str()?),
                    "inventory_opening" => ("inventory-opening", item["id"].as_str()?),
                    _ => return None,
                };
                Some(ResourceRef {
                    r#type: resource.replace('-', "_"),
                    id: Some(id.into()),
                    title: item["number"].as_str().unwrap_or("业务单据").into(),
                    biz_uri: format!("biz://{resource}/{id}"),
                })
            })
            .collect(),
        items,
        pagination: Some(Pagination {
            next_cursor: next_offset.map(|offset| offset.to_string()),
            has_more,
        }),
        evidence: vec![],
        warnings: vec![],
        trace_id: context.trace_id,
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn lookup_forwards_identity_and_preserves_source_pagination() {
        use axum::extract::Query;
        use std::collections::HashMap;
        let actor = Uuid::new_v4();
        let trace = Uuid::new_v4();
        let server = Router::new().route("/v1/agent-stock-documents/shipment", get(move |headers: HeaderMap, Query(query): Query<HashMap<String,String>>| async move {
            assert_eq!(headers["x-enterprise-user-id"], actor.to_string());
            assert_eq!(headers["x-trace-id"], trace.to_string());
            assert_eq!(headers["x-service-audience"], "business-core");
            assert_eq!(query["query"], "同名客户");
            assert_eq!(query["offset"], "5");
            assert_eq!(query["limit"], "1");
            Json(json!({"items":[{"id":"one","warehouseId":"outside"},{"id":"two","warehouseId":"allowed"}]}))
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
            required_scope: "shipment:read".into(),
            source_buzz_event_id: "a".repeat(64),
            source_channel_id: "channel".into(),
        };
        let scope = AuthorizationScope {
            warehouse_ids: ["allowed".into()].into(),
            ..Default::default()
        };
        let result = search(
            &core,
            "shipment",
            &json!({"query":"同名客户","offset":5,"limit":1}),
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
