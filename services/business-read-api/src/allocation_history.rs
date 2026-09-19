use super::*;
use business_query_contracts::{SettlementAllocationsInput, ValidateInput};

pub(super) async fn search(
    core: &CoreClient,
    kind: &str,
    input: &Value,
    scope: &AuthorizationScope,
    context: &RequestContext,
) -> Response {
    let Ok(mut input) = serde_json::from_value::<SettlementAllocationsInput>(input.clone()) else {
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
        .join(&format!("v1/agent-allocation-history/{kind}"))
    else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("offset", &input.offset.to_string())
            .append_pair("limit", &input.limit.to_string());
        query.append_pair("sourceDocumentId", &input.source_document_id.to_string());
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
    let has_more = envelope["hasMore"].as_bool().unwrap_or(false);
    let next_offset = envelope["nextOffset"].as_u64();
    let Some(rows) = envelope.get_mut("items").and_then(Value::as_array_mut) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    rows.truncate(input.limit as usize);
    let items = rows
        .iter()
        .filter(|item| permits_document(item, scope))
        .cloned()
        .collect::<Vec<_>>();
    Json(BusinessToolResult {
        schema_version: 1,
        status: BusinessToolStatus::Ok,
        as_of: chrono::Utc::now(),
        scope_summary: ScopeSummary {
            legal_entity_ids: scope.legal_entity_ids.iter().cloned().collect(),
            ..Default::default()
        },
        summary: BTreeMap::from([
            ("source".into(), json!("business-core-allocation-history")),
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
                    "customer_receipt" => ("customer-receipt", item["sourceDocumentId"].as_str()?),
                    "supplier_payment" => ("supplier-payment", item["sourceDocumentId"].as_str()?),
                    "receivable" => ("customer", item["customerId"].as_str()?),
                    "payable" => ("supplier", item["supplierId"].as_str()?),
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
        let server = Router::new().route("/v1/agent-allocation-history/customer_receipt", get(move |headers: HeaderMap, Query(query): Query<HashMap<String,String>>| async move {
            assert_eq!(headers["x-enterprise-user-id"], actor.to_string());
            assert_eq!(headers["x-trace-id"], trace.to_string());
            assert_eq!(headers["x-service-audience"], "business-core");
            assert_eq!(query["sourceDocumentId"],actor.to_string());
            assert_eq!(query["offset"], "5");
            assert_eq!(query["limit"], "1");
            Json(json!({"items":[{"id":"one","source":{"customerId":"outside"}}],"hasMore":true,"nextOffset":6}))
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
            required_scope: "customer_receipt:read".into(),
            source_buzz_event_id: "a".repeat(64),
            source_channel_id: "channel".into(),
        };
        let scope = AuthorizationScope {
            customer_ids: ["allowed".into()].into(),
            ..Default::default()
        };
        let result = search(
            &core,
            "customer_receipt",
            &json!({"sourceDocumentId":actor,"offset":5,"limit":1}),
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
