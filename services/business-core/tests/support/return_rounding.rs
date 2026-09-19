use super::*;

pub(super) async fn check(app: &Router, store: &PgStore, f: &Fixture, supplier: Uuid) {
    let sku = Uuid::new_v4();
    sqlx::query("INSERT INTO business_skus(id,product_id,code,name) SELECT $1,product_id,$3,'Return rounding fixture' FROM business_skus WHERE id=$2").bind(sku).bind(f.sku).bind(format!("ROUND-{sku}")).execute(store.pool()).await.unwrap();
    let (status,opening)=call(app,f.actor,"POST","/v1/agent-drafts/inventory-openings",json!({"legalEntityId":f.legal_entity,"businessDate":"2026-09-19","currency":"CNY","lines":[{"warehouseId":f.warehouse,"skuId":sku,"quantity":"1","unitCost":"2"}]})).await;
    assert_eq!(status, StatusCode::OK, "{opening}");
    stock_reversal_checks::confirm(
        app,
        f.actor,
        "stock/inventory_opening",
        opening["id"].as_str().unwrap(),
    )
    .await;
    let receipt = fulfill(app, f, sku, supplier, false, "2", "100").await;
    fulfill(app, f, sku, supplier, true, "1", "100").await;
    let before:(Decimal,Decimal,Decimal)=sqlx::query_as("SELECT on_hand_quantity,inventory_value,average_unit_cost FROM inventory_balances WHERE sku_id=$1").bind(sku).fetch_one(store.pool()).await.unwrap();
    assert_eq!(before.0, Decimal::from(2));
    assert_ne!(
        before.1,
        before.0 * before.2,
        "fixture must expose the moving-average rounding remainder"
    );
    let source_line: Uuid =
        sqlx::query_scalar("SELECT id FROM goods_receipt_lines WHERE goods_receipt_id=$1")
            .bind(receipt)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let (status,draft)=call(app,f.actor,"POST","/v1/agent-drafts/returns/purchase_return",json!({"sourceId":receipt,"expectedSourceVersion":2,"returnDate":"2026-09-19","reasonCode":"full return rounding","lines":[{"sourceLineId":source_line,"quantity":"2"}]})).await;
    assert_eq!(status, StatusCode::OK, "{draft}");
    let id = draft["id"].as_str().unwrap();
    let (status, preview) = call(
        app,
        f.actor,
        "GET",
        &format!("/v1/agent-approval-previews/returns/purchase_return/{id}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(
        preview["item"]["cost"]
            .as_str()
            .unwrap()
            .parse::<Decimal>()
            .unwrap(),
        before.1
    );
    let cmd = json!({"expectedVersion":1,"previewHash":preview["previewHash"],"decision":"approve","sourceBuzzEventId":Uuid::new_v4().simple().to_string().repeat(2),"sourceChannelId":"return-rounding"});
    let (status, result) = call(
        app,
        f.actor,
        "POST",
        &format!("/v1/agent-approvals/returns/purchase_return/{id}"),
        cmd,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true);
    let after:(Decimal,Decimal,Option<Decimal>)=sqlx::query_as("SELECT on_hand_quantity,inventory_value,average_unit_cost FROM inventory_balances WHERE sku_id=$1").bind(sku).fetch_one(store.pool()).await.unwrap();
    assert_eq!(after, (Decimal::ZERO, Decimal::ZERO, None));
    let movement:(Decimal,Decimal)=sqlx::query_as("SELECT quantity,total_cost FROM inventory_movements WHERE source_type='purchase_return' AND source_id=$1").bind(id.parse::<Uuid>().unwrap()).fetch_one(store.pool()).await.unwrap();
    assert_eq!(movement, (-before.0, -before.1));
    let ledger: Decimal =
        sqlx::query_scalar("SELECT sum(total_cost) FROM inventory_movements WHERE sku_id=$1")
            .bind(sku)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(
        ledger,
        Decimal::ZERO,
        "zero stock must reconcile to zero ledger cost"
    );
}

async fn fulfill(
    app: &Router,
    f: &Fixture,
    sku: Uuid,
    supplier: Uuid,
    sales: bool,
    quantity: &str,
    price: &str,
) -> Uuid {
    let resource = if sales {
        "sales-orders"
    } else {
        "purchase-orders"
    };
    let mut input = json!({"legalEntityId":f.legal_entity,"businessUnitId":f.business_unit,"currency":"CNY","orderDate":"2026-09-19","lines":[{"skuId":sku,"warehouseId":f.warehouse,"unitOfMeasureId":f.uom,"quantity":quantity,"unitPrice":price,"discountAmount":"0","taxRate":"0"}]});
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
    let id = order["id"].as_str().unwrap();
    stock_reversal_checks::confirm(app, f.actor, resource, id).await;
    let (_, detail) = call(
        app,
        f.actor,
        "GET",
        &format!("/v1/agent-documents/{resource}/{id}"),
        Value::Null,
    )
    .await;
    let (path, kind, input) = if sales {
        (
            "shipments",
            "shipment",
            json!({"salesOrderId":id,"warehouseId":f.warehouse,"shipmentDate":"2026-09-19","lines":[{"salesOrderLineId":detail["lines"][0]["id"],"quantity":quantity}]}),
        )
    } else {
        (
            "goods-receipts",
            "goods_receipt",
            json!({"purchaseOrderId":id,"warehouseId":f.warehouse,"receiptDate":"2026-09-19","lines":[{"purchaseOrderLineId":detail["lines"][0]["id"],"quantity":quantity}]}),
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
    stock_reversal_checks::confirm(app, f.actor, &format!("stock/{kind}"), id).await;
    id.parse().unwrap()
}
