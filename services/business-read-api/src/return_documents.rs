use super::*;
use business_query_contracts::{
    GetBusinessDocumentInput, SearchStockDocumentsInput, ValidateInput,
};

pub(super) fn family(tool: &str) -> Option<(&'static str, &'static str)> {
    match tool {
        "search_sales_returns" => Some(("sales_return", "search")),
        "search_purchase_returns" => Some(("purchase_return", "search")),
        "get_sales_return_source" => Some(("sales_return", "source")),
        "get_purchase_return_source" => Some(("purchase_return", "source")),
        "get_sales_return_approval_preview" => Some(("sales_return", "preview")),
        "get_purchase_return_approval_preview" => Some(("purchase_return", "preview")),
        _ => None,
    }
}
pub(super) async fn read(
    core: &CoreClient,
    kind: &str,
    mode: &str,
    input: &Value,
    scope: &AuthorizationScope,
    context: &RequestContext,
) -> Response {
    let (path, filter) = if mode == "search" {
        let Ok(mut filter) = serde_json::from_value::<SearchStockDocumentsInput>(input.clone())
        else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        if filter
            .validate_and_normalize(chrono::Utc::now().date_naive())
            .is_err()
        {
            return StatusCode::BAD_REQUEST.into_response();
        }
        (format!("v1/agent-return-documents/{kind}"), Some(filter))
    } else {
        let Ok(mut id) = serde_json::from_value::<GetBusinessDocumentInput>(input.clone()) else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        if id
            .validate_and_normalize(chrono::Utc::now().date_naive())
            .is_err()
        {
            return StatusCode::BAD_REQUEST.into_response();
        }
        (
            if mode == "source" {
                format!("v1/agent-return-sources/{kind}/{}", id.document_id)
            } else {
                format!(
                    "v1/agent-approval-previews/returns/{kind}/{}",
                    id.document_id
                )
            },
            None,
        )
    };
    let Ok(mut url) = core.base_url.join(&path) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if let Some(filter) = &filter {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("offset", &filter.offset.to_string())
            .append_pair("limit", &filter.limit.to_string());
        for (key, value) in [
            ("documentId", filter.document_id),
            ("partyId", filter.party_id),
        ] {
            if let Some(value) = value {
                query.append_pair(key, &value.to_string());
            }
        }
        for (key, value) in [("query", &filter.query), ("status", &filter.status)] {
            if let Some(value) = value {
                query.append_pair(key, value);
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
    let (mut items, has_more, next) = if let Some(filter) = &filter {
        let Some(items) = envelope["items"].as_array() else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let more = items.len() > filter.limit as usize;
        (
            items
                .iter()
                .take(filter.limit as usize)
                .cloned()
                .collect::<Vec<_>>(),
            more,
            more.then_some(filter.offset + filter.limit),
        )
    } else {
        if !envelope["item"].is_object() {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        (vec![envelope["item"].clone()], false, None)
    };
    items.retain(|item| permits_document(item, scope));
    if filter.is_none() && items.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mut summary = BTreeMap::from([
        ("source".into(), json!("business-core-returns")),
        ("resourceType".into(), json!(kind)),
        ("nextOffset".into(), json!(next)),
        (
            "requiresDisambiguation".into(),
            json!(has_more || items.len() > 1),
        ),
    ]);
    for key in ["previewHash", "approvalCommand", "rejectionCommand"] {
        if let Some(value) = envelope.get(key) {
            summary.insert(key.into(), value.clone());
        }
    }
    // Link to the existing fulfillment detail page, explicitly named as related.
    // Dedicated return detail-page links are not advertised until those routes exist.
    let refs = items
        .iter()
        .filter_map(|item| {
            let id = if mode == "source" {
                item["id"].as_str()?
            } else {
                item["sourceId"].as_str()?
            };
            let resource = if kind == "sales_return" {
                "shipment"
            } else {
                "goods-receipt"
            };
            Some(ResourceRef {
                r#type: resource.replace('-', "_"),
                id: Some(id.into()),
                title: "查看关联履约单据".into(),
                biz_uri: format!("biz://{resource}/{id}"),
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
        summary,
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
mod tests {
    use super::*;
    use axum::extract::Query;
    use std::collections::HashMap;

    #[tokio::test]
    async fn return_reads_bind_trace_scope_and_keep_raw_pagination() {
        let actor = Uuid::new_v4();
        let trace = Uuid::new_v4();
        let document = Uuid::new_v4();
        let server=Router::new()
            .route("/v1/agent-return-documents/sales_return",get(move |headers:HeaderMap,Query(query):Query<HashMap<String,String>>|async move{
                assert_eq!(headers["x-enterprise-user-id"],actor.to_string());assert_eq!(headers["x-trace-id"],trace.to_string());
                assert_eq!(query["query"],"SRET");assert_eq!(query["offset"],"2");assert_eq!(query["limit"],"1");
                Json(json!({"items":[{"id":document,"brandId":"allowed","currentBrandId":"outside"},{"id":document,"brandId":"allowed","currentBrandId":"allowed"}],"traceId":trace}))
            }))
            .route("/v1/agent-return-sources/sales_return/{id}",get(move||async move{Json(json!({"item":{"id":document,"brandId":"allowed","currentBrandId":"outside"},"traceId":trace}))}))
            .route("/v1/agent-approval-previews/returns/purchase_return/{id}",get(move||async move{Json(json!({"item":{"id":document},"traceId":Uuid::new_v4()}))}));
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
            required_scope: "sales_return:read".into(),
            source_buzz_event_id: "a".repeat(64),
            source_channel_id: "channel".into(),
        };
        let scope = AuthorizationScope {
            brand_ids: ["allowed".into()].into(),
            ..Default::default()
        };
        let response = read(
            &core,
            "sales_return",
            "search",
            &json!({"query":"SRET","offset":2,"limit":1}),
            &scope,
            &context,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["items"], json!([]));
        assert_eq!(body["summary"]["nextOffset"], 3);
        assert_eq!(body["pagination"]["hasMore"], true);
        assert_eq!(body["summary"]["requiresDisambiguation"], true);
        assert_eq!(
            read(
                &core,
                "sales_return",
                "source",
                &json!({"documentId":document}),
                &scope,
                &context
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            read(
                &core,
                "purchase_return",
                "preview",
                &json!({"documentId":document}),
                &scope,
                &context
            )
            .await
            .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            read(
                &core,
                "sales_return",
                "source",
                &json!({"documentId":document,"unknown":1}),
                &scope,
                &context
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        task.abort();
    }
}
