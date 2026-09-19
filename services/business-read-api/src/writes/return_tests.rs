use super::*;

#[tokio::test]
async fn return_source_scope_checks_frozen_and_current_line_brands() {
    for (tool, kind) in [
        ("create_sales_return_draft", "sales_return"),
        ("create_purchase_return_draft", "purchase_return"),
    ] {
        for allowed in [true, false] {
            let source = Uuid::new_v4();
            let actor = Uuid::new_v4();
            let trace = Uuid::new_v4();
            let server = Router::new().route(&format!("/v1/agent-return-sources/{kind}/{source}"), axum::routing::get(move |headers: HeaderMap| async move {
                assert_eq!(headers["x-enterprise-user-id"], actor.to_string());
                assert_eq!(headers["x-trace-id"], trace.to_string());
                assert_eq!(headers["x-service-audience"], "business-core");
                Json(json!({"traceId":trace,"item":{"id":source,"legalEntityId":"cn","businessUnitId":"unit","warehouseId":"warehouse","brandId":"brand","lines":[{"brandId":"brand","currentBrandId":if allowed {"brand"} else {"outside"}}]}}))
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let task = tokio::spawn(async move {
                axum::serve(listener, server).await.unwrap();
            });
            let core = CoreClient {
                client: reqwest::Client::new(),
                base_url: Url::parse(&format!("http://{address}/")).unwrap(),
                credential: "c".repeat(32),
            };
            let context = RequestContext {
                enterprise_user_id: actor,
                identity_binding_id: Uuid::new_v4(),
                delegation_id: Uuid::new_v4(),
                agent_id: "test".into(),
                agent_turn_id: "test".into(),
                trace_id: trace,
                used_calls: 1,
                required_scope: format!("{kind}:create"),
                source_buzz_event_id: "a".repeat(64),
                source_channel_id: "test".into(),
            };
            let grant = EffectiveGrant {
                capability: business_iam::Capability::parse(&context.required_scope).unwrap(),
                data_scope: DataScope::Restricted(BTreeMap::from([
                    ("legal_entity".into(), ["cn".into()].into()),
                    ("business_unit".into(), ["unit".into()].into()),
                    ("warehouse".into(), ["warehouse".into()].into()),
                    ("brand".into(), ["brand".into()].into()),
                ])),
                obligations: Default::default(),
            };
            assert_eq!(
                scope_allows_write(&core, tool, &json!({"sourceId":source}), &context, &grant)
                    .await,
                allowed
            );
            task.abort();
        }
    }
}
