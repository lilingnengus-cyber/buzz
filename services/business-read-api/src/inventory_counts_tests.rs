use super::*;
use axum::extract::Query;
use std::collections::HashMap;

fn item(id: Uuid, unit: Uuid, brand: Uuid) -> Value {
    json!({"id":id,"legalEntityId":id,"warehouseId":id,"businessUnitId":unit,"snapshotBusinessUnitId":unit,
        "lines":[{"brandId":brand,"snapshotBrandId":brand}]})
}
#[test]
fn count_scope_checks_current_and_frozen_dimensions_without_party_scope() {
    let id = Uuid::new_v4();
    let unit = Uuid::new_v4();
    let brand = Uuid::new_v4();
    let scope = AuthorizationScope {
        legal_entity_ids: [id.to_string()].into(),
        warehouse_ids: [id.to_string()].into(),
        business_unit_ids: [unit.to_string()].into(),
        brand_ids: [brand.to_string()].into(),
        customer_ids: [Uuid::new_v4().to_string()].into(),
        supplier_ids: [Uuid::new_v4().to_string()].into(),
    };
    let value = item(id, unit, brand);
    assert!(permits(&value, &scope, false));
    for field in [
        "legalEntityId",
        "warehouseId",
        "businessUnitId",
        "snapshotBusinessUnitId",
    ] {
        let mut bad = value.clone();
        bad[field] = json!(Uuid::new_v4());
        assert!(!permits(&bad, &scope, false), "{field}");
    }
    for field in ["brandId", "snapshotBrandId"] {
        let mut bad = value.clone();
        bad["lines"][0][field] = json!(Uuid::new_v4());
        assert!(!permits(&bad, &scope, false), "{field}");
        bad["lines"][0].as_object_mut().unwrap().remove(field);
        assert!(!permits(&bad, &scope, false));
    }
    let mut legacy = value.clone();
    legacy["snapshotBusinessUnitId"] = Value::Null;
    legacy["lines"][0]["snapshotBrandId"] = Value::Null;
    assert!(permits(&legacy, &scope, false));
    let mut option = value;
    option["brandId"] = json!(brand);
    assert!(permits(&option, &scope, true));
    option["brandId"] = json!(Uuid::new_v4());
    assert!(!permits(&option, &scope, true));
    assert!(!permits(&json!({}), &AuthorizationScope::default(), false));
}

#[tokio::test]
async fn count_reads_bind_identity_trace_exact_id_and_source_pagination() {
    let actor = Uuid::new_v4();
    let trace = Uuid::new_v4();
    let id = Uuid::new_v4();
    let unit = Uuid::new_v4();
    let brand = Uuid::new_v4();
    let row = item(id, unit, brand);
    let returned = row.clone();
    let server = Router::new()
        .route(
            "/v1/agent-inventory-counts",
            get(
                move |headers: HeaderMap, Query(query): Query<HashMap<String, String>>| {
                    let mut denied = returned.clone();
                    denied["snapshotBusinessUnitId"] = json!(Uuid::new_v4());
                    async move {
                        assert_eq!(headers["x-enterprise-user-id"], actor.to_string());
                        assert_eq!(headers["x-trace-id"], trace.to_string());
                        assert_eq!(headers["x-service-audience"], "business-core");
                        assert_eq!(query["query"], "IC");
                        assert_eq!(query["limit"], "1");
                        assert_eq!(query["offset"], "2");
                        Json(json!({"items":[denied,denied],"traceId":trace}))
                    }
                },
            ),
        )
        .route(
            "/v1/agent-inventory-counts/{id}",
            get(move |Path(requested): Path<String>| {
                let returned = row.clone();
                async move {
                    if requested == id.to_string() {
                        Json(json!({"item":returned,"traceId":trace}))
                    } else {
                        Json(json!({"item":returned,"traceId":Uuid::new_v4()}))
                    }
                }
            }),
        )
        .route(
            "/v1/agent-inventory-count-options",
            get(move || async move { Json(json!({"items":[],"traceId":trace})) }),
        );
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
        required_scope: "inventory:read".into(),
        source_buzz_event_id: "a".repeat(64),
        source_channel_id: "channel".into(),
    };
    let scope = AuthorizationScope {
        business_unit_ids: [unit.to_string()].into(),
        ..Default::default()
    };
    let response = read(
        &core,
        "search_inventory_counts",
        &json!({"query":" IC ","limit":1,"offset":2}),
        &scope,
        &context,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let result: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result["items"], json!([]));
    assert_eq!(result["resourceRefs"], json!([]));
    assert_eq!(result["summary"]["nextOffset"], 3);
    assert_eq!(result["pagination"]["hasMore"], true);
    let response = read(
        &core,
        "get_inventory_count",
        &json!({"documentId":id}),
        &scope,
        &context,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let result: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        result["resourceRefs"][0]["bizUri"],
        format!("biz://inventory-count/{id}")
    );
    let denied = AuthorizationScope {
        business_unit_ids: [Uuid::new_v4().to_string()].into(),
        ..Default::default()
    };
    assert_eq!(
        read(
            &core,
            "get_inventory_count",
            &json!({"documentId":id}),
            &denied,
            &context
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        read(
            &core,
            "get_inventory_count",
            &json!({"documentId":Uuid::new_v4()}),
            &scope,
            &context
        )
        .await
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    for tool in [
        "search_inventory_counts",
        "get_inventory_count",
        "search_inventory_count_options",
    ] {
        assert_eq!(required_capability(tool), Some("inventory:read"));
        assert!(READ_TOOLS.contains(&tool));
        assert_eq!(
            read(&core, tool, &json!({"execute":true}), &scope, &context)
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let response = read(
        &core,
        "search_inventory_count_options",
        &json!({}),
        &scope,
        &context,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    task.abort();
}
