use super::*;
use sqlx::Row;

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

pub(super) async fn check(app: &Router, store: &PgStore, f: &Fixture, supplier: Uuid) {
    for kind in ["shipment", "goods_receipt", "inventory_opening"] {
        let action = format!("{kind}:reverse");
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,$2 FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).bind(&action).execute(store.pool()).await.unwrap();
        let sku = Uuid::new_v4();
        sqlx::query("INSERT INTO business_skus(id,product_id,code,name) SELECT $1,product_id,$3,'Stock reversal fixture' FROM business_skus WHERE id=$2").bind(sku).bind(f.sku).bind(format!("REV-{sku}")).execute(store.pool()).await.unwrap();
        let (status,opening)=call(app,f.actor,"POST","/v1/agent-drafts/inventory-openings",json!({"legalEntityId":f.legal_entity,"businessDate":"2026-09-19","currency":"CNY","lines":[{"warehouseId":f.warehouse,"skuId":sku,"quantity":"4","unitCost":"50"}]})).await;
        assert_eq!(status, StatusCode::OK, "{opening}");
        confirm(
            app,
            f.actor,
            "stock/inventory_opening",
            opening["id"].as_str().unwrap(),
        )
        .await;
        let id = if kind == "inventory_opening" {
            opening["id"].as_str().unwrap().to_owned()
        } else {
            let sales = kind == "shipment";
            let resource = if sales {
                "sales-orders"
            } else {
                "purchase-orders"
            };
            let mut input = json!({"legalEntityId":f.legal_entity,"businessUnitId":f.business_unit,"currency":"CNY","orderDate":"2026-09-19","lines":[{"skuId":sku,"warehouseId":f.warehouse,"unitOfMeasureId":f.uom,"quantity":"1","unitPrice":"100","discountAmount":"0","taxRate":"0"}]});
            input[if sales { "customerId" } else { "supplierId" }] =
                json!(if sales { f.customer } else { supplier });
            let (status, order) = call(
                app,
                f.actor,
                "POST",
                &format!("/v1/agent-drafts/{resource}"),
                input,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{order}");
            let order_id = order["id"].as_str().unwrap();
            confirm(app, f.actor, resource, order_id).await;
            let (_, detail) = call(
                app,
                f.actor,
                "GET",
                &format!("/v1/agent-documents/{resource}/{order_id}"),
                Value::Null,
            )
            .await;
            let (path, input) = if sales {
                (
                    "shipments",
                    json!({"salesOrderId":order_id,"warehouseId":f.warehouse,"shipmentDate":"2026-09-19","lines":[{"salesOrderLineId":detail["lines"][0]["id"],"quantity":"1"}]}),
                )
            } else {
                (
                    "goods-receipts",
                    json!({"purchaseOrderId":order_id,"warehouseId":f.warehouse,"receiptDate":"2026-09-19","lines":[{"purchaseOrderLineId":detail["lines"][0]["id"],"quantity":"1"}]}),
                )
            };
            let (status, stock) = call(
                app,
                f.actor,
                "POST",
                &format!("/v1/agent-drafts/{path}"),
                input,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{stock}");
            let id = stock["id"].as_str().unwrap();
            confirm(app, f.actor, &format!("stock/{kind}"), id).await;
            id.to_owned()
        };
        let path = format!("/v1/agent-stock-reversal-intents/{kind}_reversal_intent");
        let reason = format!("核实并撤销错误履约 {kind}");
        let input = json!({"sourceDocumentId":id,"expectedSourceVersion":2,"reason":reason});
        let key = Uuid::new_v4().to_string();
        let (status, preview) = call_key(app, f.actor, "POST", &path, input.clone(), &key).await;
        assert_eq!(status, StatusCode::OK, "{preview}");
        let (_, replay) = call_key(app, f.actor, "POST", &path, input.clone(), &key).await;
        assert_eq!(replay["item"]["id"], preview["item"]["id"]);
        let mut changed = input.clone();
        changed["reason"] = json!("changed");
        assert_eq!(
            call_key(app, f.actor, "POST", &path, changed, &key).await.0,
            StatusCode::CONFLICT
        );
        let mut invalid = input.clone();
        invalid["reason"] = json!("");
        assert_eq!(
            call(app, f.actor, "POST", &path, invalid).await.0,
            StatusCode::BAD_REQUEST
        );
        let intent = preview["item"]["id"].as_str().unwrap();
        let preview_path =
            format!("/v1/agent-approval-previews/stock-reversals/{kind}_reversal_intent/{intent}");
        sqlx::query(
            "DELETE FROM business_warehouse_scopes WHERE enterprise_user_id=$1 AND warehouse_id=$2",
        )
        .bind(f.actor)
        .bind(f.warehouse)
        .execute(store.pool())
        .await
        .unwrap();
        assert_eq!(
            call(app, f.actor, "GET", &preview_path, Value::Null)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        sqlx::query("INSERT INTO business_warehouse_scopes(enterprise_user_id,warehouse_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.warehouse).execute(store.pool()).await.unwrap();
        let expired = Uuid::new_v4();
        sqlx::query("INSERT INTO business_agent_stock_reversal_intents(id,kind,source_document_id,input,snapshot,created_by_user_id,idempotency_key,trace_id,expires_at) SELECT $1,kind,source_document_id,input,snapshot,created_by_user_id,$2,trace_id,now()-interval '1 second' FROM business_agent_stock_reversal_intents WHERE id=$3")
            .bind(expired).bind(format!("expired-{expired}")).bind(intent.parse::<Uuid>().unwrap()).execute(store.pool()).await.unwrap();
        assert_eq!(
            call(
                app,
                f.actor,
                "GET",
                &format!(
                    "/v1/agent-approval-previews/stock-reversals/{kind}_reversal_intent/{expired}"
                ),
                Value::Null
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        let approve =
            format!("/v1/agent-approvals/stock-reversals/{kind}_reversal_intent/{intent}");
        let command = approval(&preview);
        assert_eq!(
            call(app, f.actor, "POST", &approve, command.clone())
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval,require_distinct_business_unit) VALUES($1,$1,ARRAY['b2_operator'],1,true,false)").bind(&action).execute(store.pool()).await.unwrap();
        let mut invalid = command.clone();
        invalid["reason"] = json!("tampered");
        assert_eq!(
            call(app, f.actor, "POST", &approve, invalid).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let mut stale = command.clone();
        stale["previewHash"] = json!("0".repeat(64));
        assert_eq!(
            call(app, f.actor, "POST", &approve, stale).await.0,
            StatusCode::CONFLICT
        );
        let immutable = sqlx::query(
            "UPDATE business_agent_stock_reversal_intents SET snapshot='{}'::jsonb WHERE id=$1",
        )
        .bind(intent.parse::<Uuid>().unwrap())
        .execute(store.pool())
        .await;
        assert!(immutable.is_err());
        let returns =
            business_core::b2::ReturnService::new(store.clone(), "SR".into(), "PR".into());
        let returned_id = if kind != "inventory_opening" {
            let input:business_core::b2::CreateReturn=serde_json::from_value(json!({"sourceId":id,"returnDate":"2026-09-19","reasonCode":"test","lines":[{"sourceLineId":preview["document"]["lines"][0]["id"],"quantity":"1"}]})).unwrap();
            if kind == "goods_receipt" {
                sqlx::query("DELETE FROM business_supplier_scopes WHERE enterprise_user_id=$1 AND supplier_id=$2").bind(f.actor).bind(supplier).execute(store.pool()).await.unwrap();
                let denied = returns
                    .create_purchase_return(f.actor, Uuid::new_v4(), "stock-return-denied", &input)
                    .await;
                assert!(matches!(
                    denied,
                    Err(business_core::b2::DomainError::NotFoundOrForbidden)
                ));
                sqlx::query("INSERT INTO business_supplier_scopes(enterprise_user_id,supplier_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(supplier).execute(store.pool()).await.unwrap();
            }
            let key = format!("stock-reversal-return-{kind}");
            let returned = if kind == "shipment" {
                returns
                    .create_sales_return(f.actor, Uuid::new_v4(), &key, &input)
                    .await
            } else {
                returns
                    .create_purchase_return(f.actor, Uuid::new_v4(), &key, &input)
                    .await
            }
            .unwrap();
            assert_eq!(
                call(app, f.actor, "POST", &approve, command.clone())
                    .await
                    .0,
                StatusCode::BAD_REQUEST
            );
            let key = format!("stock-reversal-return-cancel-{kind}");
            let input = VersionCommand {
                expected_version: 1,
                reason_code: Some("test cleanup".into()),
            };
            if kind == "shipment" {
                returns
                    .cancel_sales_return(f.actor, Uuid::new_v4(), returned.id, &key, &input)
                    .await
            } else {
                returns
                    .cancel_purchase_return(f.actor, Uuid::new_v4(), returned.id, &key, &input)
                    .await
            }
            .unwrap();
            Some(returned.id)
        } else {
            None
        };
        race_balance_change(app, store, f, sku, &approve, command).await;
        // Restore fixture balance and prepare a new intent after failed execution.
        sqlx::query("UPDATE inventory_balances SET reserved_quantity=reserved_quantity-0.25 WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3").bind(f.legal_entity).bind(f.warehouse).bind(sku).execute(store.pool()).await.unwrap();
        let (status, preview) = call(app, f.actor, "POST", &path, input).await;
        assert_eq!(status, StatusCode::OK, "{preview}");
        let intent = preview["item"]["id"].as_str().unwrap();
        let approve =
            format!("/v1/agent-approvals/stock-reversals/{kind}_reversal_intent/{intent}");
        let (status, result) = call(app, f.actor, "POST", &approve, approval(&preview)).await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(result["executed"], true);
        assert_eq!(
            call(app, f.actor, "POST", &approve, approval(&preview))
                .await
                .0,
            StatusCode::CONFLICT
        );
        let row=sqlx::query("SELECT on_hand_quantity,reserved_quantity,inventory_value FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3").bind(f.legal_entity).bind(f.warehouse).bind(sku).fetch_one(store.pool()).await.unwrap();
        let effect = &preview["document"]["inventoryEffects"][0];
        for (column, key) in [
            ("on_hand_quantity", "onHandQuantityAfter"),
            ("reserved_quantity", "reservedQuantityAfter"),
            ("inventory_value", "inventoryValueAfter"),
        ] {
            assert_eq!(
                row.get::<Decimal, _>(column),
                effect[key].as_str().unwrap().parse::<Decimal>().unwrap()
            );
        }
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM business_core_audit_events WHERE target_id=$1 AND details->>'reason'=$2").bind(&id).bind(&reason).fetch_one(store.pool()).await.unwrap();
        assert_eq!(count, 1);
        if let Some(returned_id) = returned_id {
            // Simulate a return draft that became visible after its source was reversed.
            let sql = if kind == "shipment" {
                "UPDATE sales_returns SET status='draft' WHERE id=$1 RETURNING version"
            } else {
                "UPDATE purchase_returns SET status='draft' WHERE id=$1 RETURNING version"
            };
            let version = sqlx::query_scalar(sql)
                .bind(returned_id)
                .fetch_one(store.pool())
                .await
                .unwrap();
            let input = VersionCommand {
                expected_version: version,
                reason_code: None,
            };
            let key = format!("stock-reversal-stale-return-{kind}");
            let result = if kind == "shipment" {
                returns
                    .confirm_sales_return(f.actor, Uuid::new_v4(), returned_id, &key, &input)
                    .await
            } else {
                returns
                    .confirm_purchase_return(f.actor, Uuid::new_v4(), returned_id, &key, &input)
                    .await
            };
            assert!(
                matches!(result,Err(business_core::b2::DomainError::Invalid(ref message)) if message.contains("reversed source")),
                "{result:?}"
            );
        }
    }
}
fn approval(preview: &Value) -> Value {
    json!({"expectedVersion":1,"previewHash":preview["previewHash"],"decision":"approve","sourceBuzzEventId":Uuid::new_v4().simple().to_string().repeat(2),"sourceChannelId":"stock-reversal-fixture"})
}
async fn race_balance_change(
    app: &Router,
    store: &PgStore,
    f: &Fixture,
    sku: Uuid,
    path: &str,
    command: Value,
) {
    let mut tx = store.pool().begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT 1 FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3 FOR UPDATE").bind(f.legal_entity).bind(f.warehouse).bind(sku).fetch_one(&mut *tx).await.unwrap();
    let app = app.clone();
    let actor = f.actor;
    let path = path.to_owned();
    let task = tokio::spawn(async move { call(&app, actor, "POST", &path, command).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(blocker)
            .fetch_one(store.pool())
            .await
            .unwrap();
            if waiting {
                break;
            }
            assert!(
                !task.is_finished(),
                "approval must reach transaction guard and wait"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    sqlx::query("UPDATE inventory_balances SET reserved_quantity=reserved_quantity+0.25 WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3").bind(f.legal_entity).bind(f.warehouse).bind(sku).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let (status, result) = tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, StatusCode::CONFLICT, "{result}");
    assert_ne!(result["executed"], true);
}
