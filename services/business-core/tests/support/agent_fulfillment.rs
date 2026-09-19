#[path = "agent_returns.rs"]
mod agent_return_checks;
#[path = "agent_order_cancellation.rs"]
mod cancellation_checks;
#[path = "return_concurrency.rs"]
mod return_concurrency_checks;
#[path = "agent_return_confirmation.rs"]
mod return_confirmation_checks;
#[path = "agent_return_disposition.rs"]
mod return_disposition_checks;
#[path = "return_draft_edit.rs"]
mod return_draft_edit_checks;
#[path = "return_logistics.rs"]
mod return_logistics_checks;
#[path = "return_rounding.rs"]
mod return_rounding_checks;
#[path = "agent_settlement.rs"]
mod settlement_checks;
#[path = "agent_stock_reversal.rs"]
mod stock_reversal_checks;

use super::*;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use business_core::{
    api::{router, AppState},
    Config,
};
use serde_json::{json, Value};
use tower::ServiceExt;

const CREDENTIAL: &str = "agent-fulfillment-test-credential-123456789";

async fn call(
    app: &Router,
    actor: Uuid,
    method: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    call_key(
        app,
        actor,
        method,
        path,
        body,
        &format!("fulfillment-test-{}", Uuid::new_v4()),
    )
    .await
}

async fn call_key(
    app: &Router,
    actor: Uuid,
    method: &str,
    path: &str,
    body: Value,
    key: &str,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("x-business-service-credential", CREDENTIAL)
                .header("x-service-audience", "business-core")
                .header("x-enterprise-user-id", actor.to_string())
                .header("x-trace-id", Uuid::new_v4().to_string())
                .header("idempotency-key", key)
                .header("content-type", "application/json")
                .body(if method == "GET" {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"raw":String::from_utf8_lossy(&bytes)})),
    )
}

pub(super) async fn check(store: &PgStore, f: &Fixture) {
    let app = router(AppState::new(
        store.clone(),
        &config("test".into(), CREDENTIAL.into()),
    ));
    let sku = Uuid::new_v4();
    sqlx::query("INSERT INTO business_skus(id,product_id,code,name) SELECT $1,product_id,'AGENT-FULFILLMENT','Agent fulfillment' FROM business_skus WHERE id=$2").bind(sku).bind(f.sku).execute(store.pool()).await.unwrap();
    let opening = json!({"legalEntityId":f.legal_entity,"businessDate":"2026-09-19","currency":"CNY","lines":[{"warehouseId":f.warehouse,"skuId":sku,"quantity":"2","unitCost":"50"}]});
    let (status, created) = call(
        &app,
        f.actor,
        "POST",
        "/v1/agent-drafts/inventory-openings",
        opening,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap();
    let preview_path = format!("/v1/agent-approval-previews/stock/inventory_opening/{id}");
    let (status, preview) = call(&app, f.actor, "GET", &preview_path, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["item"]["lines"][0]["quantity"], "2.000000");
    assert!(preview["approvalCommand"]
        .as_str()
        .unwrap()
        .starts_with("确认 inventory-opening "));
    let command = json!({"expectedVersion":1,"previewHash":preview["previewHash"],"decision":"approve","sourceBuzzEventId":"a".repeat(64),"sourceChannelId":"test-fulfillment"});
    let path = format!("/v1/agent-approvals/stock/inventory_opening/{id}");
    let (status, _) = call(&app, f.actor, "POST", &path, command.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "missing policy must deny");
    sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval,require_distinct_business_unit) VALUES('inventory_opening:post','inventory_opening:post',ARRAY['b2_operator'],1,true,false)").execute(store.pool()).await.unwrap();
    let mut stale = command.clone();
    stale["previewHash"] = json!("0".repeat(64));
    let (status, _) = call(&app, f.actor, "POST", &path, stale).await;
    assert_eq!(status, StatusCode::CONFLICT);
    // A current user scope revoke prevents execution, even with a previously valid preview.
    sqlx::query(
        "DELETE FROM business_warehouse_scopes WHERE enterprise_user_id=$1 AND warehouse_id=$2",
    )
    .bind(f.actor)
    .bind(f.warehouse)
    .execute(store.pool())
    .await
    .unwrap();
    let (status, _) = call(&app, f.actor, "POST", &path, command.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    sqlx::query("INSERT INTO business_warehouse_scopes(enterprise_user_id,warehouse_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.warehouse).execute(store.pool()).await.unwrap();
    let (status, result) = call(&app, f.actor, "POST", &path, command.clone()).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true);
    let (status, _) = call(&app, f.actor, "POST", &path, command).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let quantity: Decimal =
        sqlx::query_scalar("SELECT on_hand_quantity FROM inventory_balances WHERE sku_id=$1")
            .bind(sku)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(quantity, Decimal::from(2));
    for permission in [
        "purchase_order:read",
        "purchase_order:create",
        "purchase_order:update_draft",
        "purchase_order:confirm",
        "goods_receipt:read",
        "goods_receipt:create",
        "goods_receipt:confirm",
        "payable:read",
    ] {
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,$2 FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).bind(permission).execute(store.pool()).await.unwrap();
    }
    for action in [
        "sales_order:confirm",
        "shipment:confirm",
        "purchase_order:confirm",
        "goods_receipt:confirm",
    ] {
        sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval,require_distinct_business_unit) VALUES($1,$1,ARRAY['b2_operator'],1,true,false) ON CONFLICT(action_code) DO NOTHING").bind(action).execute(store.pool()).await.unwrap();
    }
    let line = json!({"skuId":sku,"warehouseId":f.warehouse,"unitOfMeasureId":f.uom,"quantity":"2","unitPrice":"100","discountAmount":"0","taxRate":"0"});
    let draft = json!({"legalEntityId":f.legal_entity,"customerId":f.customer,"businessUnitId":f.business_unit,"currency":"CNY","orderDate":"2026-09-19","lines":[line.clone()]});
    let (status, order) = call(
        &app,
        f.actor,
        "POST",
        "/v1/agent-drafts/sales-orders",
        draft.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{order}");
    let order_id = order["id"].as_str().unwrap();
    let (_, detail) = call(
        &app,
        f.actor,
        "GET",
        &format!("/v1/agent-documents/sales-orders/{order_id}"),
        Value::Null,
    )
    .await;
    assert_eq!(detail["lines"][0]["orderedQuantity"], "2.000000");
    let mut replacement = draft;
    replacement.as_object_mut().unwrap().remove("legalEntityId");
    replacement["expectedVersion"] = json!(1);
    replacement["businessNote"] = json!("confirmed edit");
    let (status, updated) = call(
        &app,
        f.actor,
        "PUT",
        &format!("/v1/agent-drafts/sales-orders/{order_id}"),
        replacement.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["version"], 2);
    let (status, _) = call(
        &app,
        f.actor,
        "PUT",
        &format!("/v1/agent-drafts/sales-orders/{order_id}"),
        replacement,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    approve(&app, f.actor, "sales-orders", order_id, 'b').await;
    let (_, detail) = call(
        &app,
        f.actor,
        "GET",
        &format!("/v1/agent-documents/sales-orders/{order_id}"),
        Value::Null,
    )
    .await;
    let (status,shipment)=call(&app,f.actor,"POST","/v1/agent-drafts/shipments",json!({"salesOrderId":order_id,"warehouseId":f.warehouse,"shipmentDate":"2026-09-19","lines":[{"salesOrderLineId":detail["lines"][0]["id"],"quantity":"2"}]})).await;
    assert_eq!(status, StatusCode::OK, "{shipment}");
    approve(
        &app,
        f.actor,
        "stock/shipment",
        shipment["id"].as_str().unwrap(),
        'c',
    )
    .await;
    let quantity: Decimal =
        sqlx::query_scalar("SELECT on_hand_quantity FROM inventory_balances WHERE sku_id=$1")
            .bind(sku)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(quantity, Decimal::ZERO);
    let supplier = Uuid::new_v4();
    sqlx::query("INSERT INTO business_suppliers(id,legal_entity_id,business_unit_id,code,name,payment_terms_days) VALUES($1,$2,$3,'AGENT-SUP','Agent supplier',30)").bind(supplier).bind(f.legal_entity).bind(f.business_unit).execute(store.pool()).await.unwrap();
    sqlx::query("INSERT INTO business_supplier_scopes(enterprise_user_id,supplier_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(supplier).execute(store.pool()).await.unwrap();
    let purchase = json!({"legalEntityId":f.legal_entity,"supplierId":supplier,"businessUnitId":f.business_unit,"currency":"CNY","orderDate":"2026-09-19","lines":[line]});
    let (status, order) = call(
        &app,
        f.actor,
        "POST",
        "/v1/agent-drafts/purchase-orders",
        purchase.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{order}");
    let order_id = order["id"].as_str().unwrap();
    let mut replacement = purchase;
    replacement["expectedVersion"] = json!(1);
    replacement["lines"][0]["unitPrice"] = json!("40");
    let (status, updated) = call(
        &app,
        f.actor,
        "PUT",
        &format!("/v1/agent-drafts/purchase-orders/{order_id}"),
        replacement,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    approve(&app, f.actor, "purchase-orders", order_id, 'd').await;
    let (_, detail) = call(
        &app,
        f.actor,
        "GET",
        &format!("/v1/agent-documents/purchase-orders/{order_id}"),
        Value::Null,
    )
    .await;
    let (status,receipt)=call(&app,f.actor,"POST","/v1/agent-drafts/goods-receipts",json!({"purchaseOrderId":order_id,"warehouseId":f.warehouse,"receiptDate":"2026-09-19","lines":[{"purchaseOrderLineId":detail["lines"][0]["id"],"quantity":"2"}]})).await;
    assert_eq!(status, StatusCode::OK, "{receipt}");
    approve(
        &app,
        f.actor,
        "stock/goods_receipt",
        receipt["id"].as_str().unwrap(),
        'e',
    )
    .await;
    let quantity: Decimal =
        sqlx::query_scalar("SELECT on_hand_quantity FROM inventory_balances WHERE sku_id=$1")
            .bind(sku)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(quantity, Decimal::from(2));
    shortage_retry(&app, store, f).await;
    settlement_checks::check(&app, store, f, supplier).await;
    cancellation_checks::check(&app, store, f, supplier, sku).await;
    stock_reversal_checks::check(&app, store, f, supplier).await;
    return_logistics_checks::check(&app, store, f, supplier).await;
    return_rounding_checks::check(&app, store, f, supplier).await;
}

async fn shortage_retry(app: &Router, store: &PgStore, f: &Fixture) {
    let sku = Uuid::new_v4();
    sqlx::query("INSERT INTO business_skus(id,product_id,code,name) SELECT $1,product_id,'AGENT-RETRY','Retry fixture' FROM business_skus WHERE id=$2").bind(sku).bind(f.sku).execute(store.pool()).await.unwrap();
    let (_,order)=call(app,f.actor,"POST","/v1/agent-drafts/sales-orders",json!({"legalEntityId":f.legal_entity,"customerId":f.customer,"businessUnitId":f.business_unit,"currency":"CNY","orderDate":"2026-09-19","lines":[{"skuId":sku,"warehouseId":f.warehouse,"unitOfMeasureId":f.uom,"quantity":"1","unitPrice":"100","discountAmount":"0","taxRate":"0"}]})).await;
    let id = order["id"].as_str().unwrap();
    let (_, preview) = call(
        app,
        f.actor,
        "GET",
        &format!("/v1/agent-approval-previews/sales-orders/{id}"),
        Value::Null,
    )
    .await;
    assert_eq!(preview["item"]["allAvailable"], false);
    let (status,_)=call(app,f.actor,"POST",&format!("/v1/agent-approvals/sales-orders/{id}"),json!({"expectedVersion":1,"previewHash":preview["previewHash"],"decision":"approve","sourceBuzzEventId":"f".repeat(64),"sourceChannelId":"test-fulfillment"})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_,opening)=call(app,f.actor,"POST","/v1/agent-drafts/inventory-openings",json!({"legalEntityId":f.legal_entity,"businessDate":"2026-09-19","currency":"CNY","lines":[{"warehouseId":f.warehouse,"skuId":sku,"quantity":"1","unitCost":"50"}]})).await;
    approve(
        app,
        f.actor,
        "stock/inventory_opening",
        opening["id"].as_str().unwrap(),
        '1',
    )
    .await;
    let (_, new_preview) = call(
        app,
        f.actor,
        "GET",
        &format!("/v1/agent-approval-previews/sales-orders/{id}"),
        Value::Null,
    )
    .await;
    assert_ne!(preview["previewHash"], new_preview["previewHash"]);
    approve(app, f.actor, "sales-orders", id, '2').await;
}

async fn approve(app: &Router, actor: Uuid, kind: &str, id: &str, event: char) {
    let (status, preview) = call(
        app,
        actor,
        "GET",
        &format!("/v1/agent-approval-previews/{kind}/{id}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    let version = preview["item"]["version"].clone();
    let command = json!({"expectedVersion":version,"previewHash":preview["previewHash"],"decision":"approve","sourceBuzzEventId":event.to_string().repeat(64),"sourceChannelId":"test-fulfillment"});
    let (status, result) = call(
        app,
        actor,
        "POST",
        &format!("/v1/agent-approvals/{kind}/{id}"),
        command,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true, "{result}");
}

fn config(database_url: String, service_credential: String) -> Config {
    Config {
        database_url,
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        service_credential,
        service_audience: "business-core".into(),
        bootstrap_enabled: false,
        bootstrap_user_id: None,
        sales_enabled: true,
        inventory_enabled: true,
        receivables_enabled: true,
        purchasing_enabled: true,
        receiving_enabled: true,
        payables_enabled: true,
        profitability_enabled: true,
        management_reporting_enabled: true,
        operational_adjustments_enabled: true,
        profit_projection_worker_enabled: true,
        profit_projection_batch_size: 200,
        profit_projection_retry_limit: 5,
        sales_order_number_prefix: "SO".into(),
        shipment_number_prefix: "SHP".into(),
        receivable_number_prefix: "AR".into(),
        customer_receipt_number_prefix: "RCPT".into(),
        inventory_opening_number_prefix: "OPEN".into(),
        inventory_count_number_prefix: "CNT".into(),
        purchase_requisition_number_prefix: "PRQ".into(),
        purchase_order_number_prefix: "PO".into(),
        goods_receipt_number_prefix: "GR".into(),
        trade_payable_number_prefix: "AP".into(),
        supplier_payment_number_prefix: "PAY".into(),
        sales_return_number_prefix: "SRET".into(),
        purchase_return_number_prefix: "PRET".into(),
        profit_adjustment_number_prefix: "ADJ".into(),
        management_report_snapshot_number_prefix: "MGR".into(),
        profit_management_timezone: "Asia/Shanghai".into(),
        profit_default_currency: "CNY".into(),
        profit_allocation_max_targets: 500,
        profit_report_max_rows: 1000,
        profit_data_stale_after_minutes: 15,
        default_payment_terms_days: 30,
        default_supplier_payment_terms_days: 30,
        default_currency: "CNY".into(),
        command_rate_limit_per_minute: 60,
        business_web_origin: "https://business.example.test".into(),
        business_web_embed_origin: "https://business.example.test".into(),
        business_session_cookie_name: "__Host-bizfin_business".into(),
    }
}
