use super::*;

async fn confirm(app: &Router, actor: Uuid, kind: &str, id: &str) {
    let (_, preview) = call(
        app,
        actor,
        "GET",
        &format!("/v1/agent-approval-previews/{kind}/{id}"),
        Value::Null,
    )
    .await;
    let (status,result)=call(app,actor,"POST",&format!("/v1/agent-approvals/{kind}/{id}"),json!({"expectedVersion":preview["item"]["version"],"previewHash":preview["previewHash"],"decision":"approve","sourceBuzzEventId":Uuid::new_v4().simple().to_string().repeat(2),"sourceChannelId":"cancellation-fixture"})).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true);
}

pub(super) async fn check(app: &Router, store: &PgStore, f: &Fixture, supplier: Uuid, sku: Uuid) {
    for (kind, resource, party_key, party, action) in [
        (
            "sales_order",
            "sales-orders",
            "customerId",
            f.customer,
            "sales_order:cancel",
        ),
        (
            "purchase_order",
            "purchase-orders",
            "supplierId",
            supplier,
            "purchase_order:cancel_remaining",
        ),
    ] {
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,$2 FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).bind(action).execute(store.pool()).await.unwrap();
        for stage in 0..3 {
            let quantity = if stage == 2 { "1" } else { "2" };
            let mut input = json!({"legalEntityId":f.legal_entity,"businessUnitId":f.business_unit,"currency":"CNY","orderDate":"2026-09-19","lines":[{"skuId":sku,"warehouseId":f.warehouse,"unitOfMeasureId":f.uom,"quantity":quantity,"unitPrice":"100","discountAmount":"0","taxRate":"0"}]});
            input[party_key] = json!(party);
            let (status, created) = call(
                app,
                f.actor,
                "POST",
                &format!("/v1/agent-drafts/{resource}"),
                input,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{created}");
            let id = created["id"].as_str().unwrap();
            if stage > 0 {
                confirm(app, f.actor, resource, id).await;
                let (_, detail) = call(
                    app,
                    f.actor,
                    "GET",
                    &format!("/v1/agent-documents/{resource}/{id}"),
                    Value::Null,
                )
                .await;
                let (path, stock, input) = if kind == "sales_order" {
                    (
                        "shipments",
                        "shipment",
                        json!({"salesOrderId":id,"warehouseId":f.warehouse,"shipmentDate":"2026-09-19","lines":[{"salesOrderLineId":detail["lines"][0]["id"],"quantity":"1"}]}),
                    )
                } else {
                    (
                        "goods-receipts",
                        "goods_receipt",
                        json!({"purchaseOrderId":id,"warehouseId":f.warehouse,"receiptDate":"2026-09-19","lines":[{"purchaseOrderLineId":detail["lines"][0]["id"],"quantity":"1"}]}),
                    )
                };
                let (status, created) = call(
                    app,
                    f.actor,
                    "POST",
                    &format!("/v1/agent-drafts/{path}"),
                    input,
                )
                .await;
                assert_eq!(status, StatusCode::OK, "{created}");
                confirm(
                    app,
                    f.actor,
                    &format!("stock/{stock}"),
                    created["id"].as_str().unwrap(),
                )
                .await;
            }
            let (_, detail) = call(
                app,
                f.actor,
                "GET",
                &format!("/v1/agent-documents/{resource}/{id}"),
                Value::Null,
            )
            .await;
            let intent_kind = format!("{kind}_cancellation_intent");
            let path = format!("/v1/agent-order-cancellation-intents/{intent_kind}");
            let reason = format!("取消剩余采购或销售需求 {}", Uuid::new_v4());
            let input = json!({"sourceDocumentId":id,"expectedSourceVersion":detail["version"],"reason":reason});
            let key = Uuid::new_v4().to_string();
            let (status, preview) =
                call_key(app, f.actor, "POST", &path, input.clone(), &key).await;
            if stage == 2 {
                assert_eq!(
                    status,
                    StatusCode::CONFLICT,
                    "fully fulfilled order cannot cancel"
                );
                continue;
            }
            assert_eq!(status, StatusCode::OK, "{preview}");
            assert_eq!(
                preview["document"]["cancelQuantity"],
                if stage == 0 { "2.000000" } else { "1.000000" }
            );
            assert_eq!(
                preview["document"]["resultStatus"],
                if stage == 0 { "cancelled" } else { "completed" }
            );
            let (_, replayed) = call_key(app, f.actor, "POST", &path, input.clone(), &key).await;
            assert_eq!(preview["item"]["id"], replayed["item"]["id"]);
            let mut changed = input;
            changed["reason"] = json!("different");
            assert_eq!(
                call_key(app, f.actor, "POST", &path, changed, &key).await.0,
                StatusCode::CONFLICT
            );
            let intent = preview["item"]["id"].as_str().unwrap();
            let approve_path =
                format!("/v1/agent-approvals/order-cancellations/{intent_kind}/{intent}");
            let command = json!({"expectedVersion":1,"previewHash":preview["previewHash"],"decision":"approve","sourceBuzzEventId":Uuid::new_v4().simple().to_string().repeat(2),"sourceChannelId":"cancellation-fixture"});
            if stage == 0 {
                assert_eq!(
                    call(app, f.actor, "POST", &approve_path, command.clone())
                        .await
                        .0,
                    StatusCode::NOT_FOUND
                );
                sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES($1,$1,ARRAY['b2_operator'],1,true)").bind(action).execute(store.pool()).await.unwrap();
            }
            let mut tampered = command.clone();
            tampered["reason"] = json!("changed at confirmation");
            assert_eq!(
                call(app, f.actor, "POST", &approve_path, tampered).await.0,
                StatusCode::UNPROCESSABLE_ENTITY
            );
            let mut stale = command.clone();
            stale["previewHash"] = json!("0".repeat(64));
            assert_eq!(
                call(app, f.actor, "POST", &approve_path, stale).await.0,
                StatusCode::CONFLICT
            );
            let before:(Decimal,Decimal)=sqlx::query_as("SELECT on_hand_quantity,reserved_quantity FROM inventory_balances WHERE sku_id=$1 AND warehouse_id=$2").bind(sku).bind(f.warehouse).fetch_one(store.pool()).await.unwrap();
            let (status, result) = call(app, f.actor, "POST", &approve_path, command.clone()).await;
            assert_eq!(status, StatusCode::OK, "{result}");
            assert_eq!(result["executed"], true);
            assert_eq!(
                call(app, f.actor, "POST", &approve_path, command).await.0,
                StatusCode::CONFLICT
            );
            let (_, final_order) = call(
                app,
                f.actor,
                "GET",
                &format!("/v1/agent-documents/{resource}/{id}"),
                Value::Null,
            )
            .await;
            assert_eq!(
                final_order["lifecycleStatus"],
                preview["document"]["resultStatus"]
            );
            assert_eq!(
                final_order["lines"][0]["cancelledQuantity"],
                preview["document"]["cancelQuantity"]
            );
            let after:(Decimal,Decimal)=sqlx::query_as("SELECT on_hand_quantity,reserved_quantity FROM inventory_balances WHERE sku_id=$1 AND warehouse_id=$2").bind(sku).bind(f.warehouse).fetch_one(store.pool()).await.unwrap();
            assert_eq!(
                before.0, after.0,
                "cancellation does not reverse posted stock"
            );
            assert_eq!(
                before.1 - after.1,
                if kind == "sales_order" && stage == 1 {
                    Decimal::ONE
                } else {
                    Decimal::ZERO
                }
            );
            let audits:i64=sqlx::query_scalar("SELECT count(*) FROM business_core_audit_events WHERE actor_user_id=$1 AND details->>'reasonCode'=$2").bind(f.actor).bind(reason).fetch_one(store.pool()).await.unwrap();
            assert_eq!(audits, 1);
        }
    }
}
