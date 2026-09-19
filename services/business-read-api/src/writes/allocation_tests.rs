use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[tokio::test]
async fn target_scope_is_checked_before_persisting_allocation_intent() {
    for allowed in [false, true] {
        for (tool, kind, category) in [
            (
                "prepare_sales_return_inspection",
                "sales_return_inspection_intent",
                "return-disposition",
            ),
            (
                "prepare_purchase_return_dispatch",
                "purchase_return_dispatch_intent",
                "return-disposition",
            ),
            (
                "prepare_purchase_return_acknowledgment",
                "purchase_return_acknowledgment_intent",
                "return-disposition",
            ),
            (
                "prepare_shipment_reversal",
                "shipment_reversal_intent",
                "stock-reversal",
            ),
            (
                "prepare_goods_receipt_reversal",
                "goods_receipt_reversal_intent",
                "stock-reversal",
            ),
            (
                "prepare_inventory_opening_reversal",
                "inventory_opening_reversal_intent",
                "stock-reversal",
            ),
            (
                "prepare_sales_order_cancellation",
                "sales_order_cancellation_intent",
                "order-cancellation",
            ),
            (
                "prepare_purchase_order_cancellation",
                "purchase_order_cancellation_intent",
                "order-cancellation",
            ),
            (
                "prepare_receivable_allocation",
                "receivable_allocation_intent",
                "allocation",
            ),
            (
                "prepare_customer_receipt_reversal",
                "customer_receipt_reversal_intent",
                "reversal",
            ),
            (
                "prepare_supplier_payment_reversal",
                "supplier_payment_reversal_intent",
                "reversal",
            ),
            (
                "prepare_receivable_allocation_reversal",
                "receivable_allocation_reversal_intent",
                "reversal",
            ),
            (
                "prepare_payable_allocation_reversal",
                "payable_allocation_reversal_intent",
                "reversal",
            ),
        ] {
            let writes = Arc::new(AtomicUsize::new(0));
            let snapshot = json!({"document":{"source":{"legalEntityId":"cn","fulfillmentId":Uuid::new_v4()},"allocations":[{"warehouseId":if allowed {"allowed"} else {"outside"}}]},"item":{"id":Uuid::new_v4(),"version":1}});
            let read = snapshot.clone();
            let counter = writes.clone();
            let server = Router::new()
                .route(
                    &format!("/v1/agent-{category}-previews/{kind}"),
                    axum::routing::post(move || {
                        let value = read.clone();
                        async move { Json(value) }
                    }),
                )
                .route(
                    &format!("/v1/agent-{category}-intents/{kind}"),
                    axum::routing::post(move || {
                        let value = snapshot.clone();
                        let writes = counter.clone();
                        async move {
                            writes.fetch_add(1, Ordering::SeqCst);
                            Json(value)
                        }
                    }),
                );
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
                enterprise_user_id: Uuid::new_v4(),
                identity_binding_id: Uuid::new_v4(),
                delegation_id: Uuid::new_v4(),
                agent_id: "test".into(),
                agent_turn_id: "test".into(),
                trace_id: Uuid::new_v4(),
                used_calls: 1,
                required_scope: format!("{kind}:create"),
                source_buzz_event_id: "a".repeat(64),
                source_channel_id: "test".into(),
            };
            let grant = EffectiveGrant {
                capability: business_iam::Capability::parse(&context.required_scope).unwrap(),
                data_scope: DataScope::Restricted(BTreeMap::from([
                    ("legal_entity".into(), ["cn".into()].into()),
                    ("warehouse".into(), ["allowed".into()].into()),
                ])),
                obligations: Default::default(),
            };
            let response = forward_intent_prepare(
                &core,
                tool,
                json!({"sourceDocumentId":Uuid::new_v4()}),
                &context,
                &grant,
            )
            .await;
            assert_eq!(
                response.status(),
                if allowed {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                }
            );
            assert_eq!(writes.load(Ordering::SeqCst), usize::from(allowed));
            task.abort();
        }
    }
}
