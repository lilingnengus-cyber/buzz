use super::*;
use axum::extract::Query;
use std::collections::HashMap;
#[tokio::test]
async fn filters_identity_scope_trace_and_pagination_are_preserved() {
    let actor = Uuid::new_v4();
    let trace = Uuid::new_v4();
    let id = Uuid::new_v4();
    let unit = Uuid::new_v4();
    let legal = Uuid::new_v4();
    let row =
        json!({"id":id,"legalEntityId":legal,"businessUnitId":unit,"customerId":null,"version":5});
    let summary = row.clone();
    let detail = row.clone();
    let server=Router::new().route("/v1/agent-crm-opportunities",get(move |headers:HeaderMap,Query(q):Query<HashMap<String,String>>|{
        let mut denied=summary.clone();denied["businessUnitId"]=json!(Uuid::new_v4());
        async move{
            assert_eq!(headers["x-enterprise-user-id"],actor.to_string());assert_eq!(headers["x-trace-id"],trace.to_string());
            assert_eq!(q["query"],"商机");assert_eq!(q["offset"],"2");assert_eq!(q["limit"],"1");
            Json(json!({"items":[denied],"hasMore":true,"nextOffset":3,"traceId":trace}))
        }
    })).route("/v1/agent-crm-opportunity",get(move |Query(q):Query<HashMap<String,String>>|{
        let detail=detail.clone();async move{
            assert_eq!(q["expectedVersion"],"5");assert_eq!(q["offset"],"3");
            Json(json!({"item":detail,"followups":[],"hasMore":false,"nextOffset":null,"traceId":trace}))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, server).await.unwrap() });
    let core = CoreClient {
        client: reqwest::Client::new(),
        base_url: Url::parse(&format!("http://{address}/")).unwrap(),
        credential: "test".into(),
    };
    let context = RequestContext {
        enterprise_user_id: actor,
        identity_binding_id: Uuid::new_v4(),
        delegation_id: Uuid::new_v4(),
        agent_id: "agent".into(),
        agent_turn_id: "turn".into(),
        trace_id: trace,
        used_calls: 1,
        required_scope: "crm:read".into(),
        source_buzz_event_id: "a".repeat(64),
        source_channel_id: "channel".into(),
    };
    let scope = AuthorizationScope {
        business_unit_ids: [unit.to_string()].into(),
        ..Default::default()
    };
    let result = read(
        &core,
        "search_crm_opportunities",
        &json!({"query":" 商机 ","limit":1,"offset":2}),
        &scope,
        &context,
    )
    .await;
    assert_eq!(result.status(), StatusCode::OK);
    let result: Value = serde_json::from_slice(
        &axum::body::to_bytes(result.into_body(), 65536)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result["items"], json!([]));
    assert_eq!(result["pagination"]["hasMore"], true);
    assert_eq!(result["summary"]["nextOffset"], 3);
    let input = json!({"documentId":id,"expectedVersion":5,"offset":3});
    let result = read(&core, "get_crm_opportunity", &input, &scope, &context).await;
    assert_eq!(result.status(), StatusCode::OK);
    let result: Value = serde_json::from_slice(
        &axum::body::to_bytes(result.into_body(), 65536)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        result["resourceRefs"][0]["bizUri"],
        format!("biz://crm-opportunity/{id}")
    );
    assert_eq!(result["summary"]["requiresDisambiguation"], false);
    let mut wrong = input;
    wrong["documentId"] = json!(Uuid::new_v4());
    assert_eq!(
        read(&core, "get_crm_opportunity", &wrong, &scope, &context)
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let denied = AuthorizationScope {
        legal_entity_ids: [Uuid::new_v4().to_string()].into(),
        ..Default::default()
    };
    assert!(!permits(&row, &denied));
    assert!(!permits(&json!({}), &AuthorizationScope::default()));
    task.abort();
}
