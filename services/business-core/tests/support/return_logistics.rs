use super::*;

pub(super) async fn check(app: &Router, store: &PgStore, f: &Fixture, supplier: Uuid) {
    let sku = Uuid::new_v4();
    sqlx::query("INSERT INTO business_skus(id,product_id,code,name) SELECT $1,product_id,$3,'Return logistics fixture' FROM business_skus WHERE id=$2")
        .bind(sku).bind(f.sku).bind(format!("LOG-{sku}")).execute(store.pool()).await.unwrap();
    let input = json!({"legalEntityId":f.legal_entity,"businessUnitId":f.business_unit,"supplierId":supplier,"currency":"CNY","orderDate":"2026-09-19","lines":[{"skuId":sku,"warehouseId":f.warehouse,"unitOfMeasureId":f.uom,"quantity":"2","unitPrice":"100","discountAmount":"0","taxRate":"0"}]});
    let (status, order) = call(
        app,
        f.actor,
        "POST",
        "/v1/agent-drafts/purchase-orders",
        input,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{order}");
    let order_id = order["id"].as_str().unwrap();
    stock_reversal_checks::confirm(app, f.actor, "purchase-orders", order_id).await;
    let (_, detail) = call(
        app,
        f.actor,
        "GET",
        &format!("/v1/agent-documents/purchase-orders/{order_id}"),
        Value::Null,
    )
    .await;
    let (status, receipt)=call(app,f.actor,"POST","/v1/agent-drafts/goods-receipts",json!({"purchaseOrderId":order_id,"warehouseId":f.warehouse,"receiptDate":"2026-09-19","lines":[{"purchaseOrderLineId":detail["lines"][0]["id"],"quantity":"2"}]})).await;
    assert_eq!(status, StatusCode::OK, "{receipt}");
    let receipt_id = receipt["id"].as_str().unwrap();
    stock_reversal_checks::confirm(app, f.actor, "stock/goods_receipt", receipt_id).await;
    let source_line: Uuid =
        sqlx::query_scalar("SELECT id FROM goods_receipt_lines WHERE goods_receipt_id=$1")
            .bind(receipt_id.parse::<Uuid>().unwrap())
            .fetch_one(store.pool())
            .await
            .unwrap();
    let input:business_core::b2::CreateReturn=serde_json::from_value(json!({"sourceId":receipt_id,"returnDate":"2026-09-19","reasonCode":"damaged delivery","lines":[{"sourceLineId":source_line,"quantity":"1"}]})).unwrap();
    let service = business_core::b2::ReturnService::new(store.clone(), "SR".into(), "PR".into());
    let returned = service
        .create_purchase_return(f.actor, Uuid::new_v4(), "logistics-create", &input)
        .await
        .unwrap();
    super::return_confirmation_checks::confirm(app, store, f, false, returned.id).await;
    let summary = service
        .purchase_returns(f.actor, 20)
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.id == returned.id)
        .unwrap();
    assert_eq!(summary.legal_entity_id, f.legal_entity);
    assert_eq!(summary.business_unit_id, f.business_unit);
    // No customer scope can authorize a supplier operation. Keep the fixture's
    // unrelated customer scope and assert the supplier UUID is not in it.
    let persisted_version: i64 =
        sqlx::query_scalar("SELECT version FROM purchase_returns WHERE id=$1")
            .bind(returned.id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(
        persisted_version, 2,
        "confirmation response and stored version must agree"
    );
    let wrong_scope:i64=sqlx::query_scalar("SELECT count(*) FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2").bind(f.actor).bind(supplier).fetch_one(store.pool()).await.unwrap();
    assert_eq!(wrong_scope, 0);
    let disposition = business_core::b2::ReturnDispositionService::new(store.clone());
    let dispatch = business_core::b2::DispatchPurchaseReturn {
        expected_version: 2,
        dispatch_date: NaiveDate::from_ymd_opt(2026, 9, 19).unwrap(),
        carrier: "Fixture carrier".into(),
        tracking_number: "RETURN-001".into(),
    };
    sqlx::query(
        "DELETE FROM business_unit_scopes WHERE enterprise_user_id=$1 AND business_unit_id=$2",
    )
    .bind(f.actor)
    .bind(f.business_unit)
    .execute(store.pool())
    .await
    .unwrap();
    assert!(matches!(
        disposition
            .dispatch_purchase_return(
                f.actor,
                Uuid::new_v4(),
                returned.id,
                "logistics-unit-denied",
                &dispatch
            )
            .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.business_unit).execute(store.pool()).await.unwrap();
    revoke(store, f.actor, supplier).await;
    assert!(matches!(
        disposition
            .dispatch_purchase_return(
                f.actor,
                Uuid::new_v4(),
                returned.id,
                "logistics-dispatch",
                &dispatch
            )
            .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    restore(store, f.actor, supplier).await;
    let dispatch_key = super::return_disposition_checks::execute(
        app,
        store,
        f,
        returned.id,
        "purchase_return_dispatch_intent",
        serde_json::to_value(&dispatch).unwrap(),
    )
    .await;
    let result = disposition
        .dispatch_purchase_return(
            f.actor,
            Uuid::new_v4(),
            returned.id,
            &dispatch_key,
            &dispatch,
        )
        .await
        .unwrap();
    assert_eq!(result.status, "dispatched");
    assert_eq!(result.version, 3);
    assert!(
        disposition
            .dispatch_purchase_return(
                f.actor,
                Uuid::new_v4(),
                returned.id,
                &dispatch_key,
                &dispatch
            )
            .await
            .unwrap()
            .idempotent_replay
    );
    // A cached success must also be inaccessible after permission withdrawal.
    revoke(store, f.actor, supplier).await;
    assert!(matches!(
        disposition
            .dispatch_purchase_return(
                f.actor,
                Uuid::new_v4(),
                returned.id,
                &dispatch_key,
                &dispatch
            )
            .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    restore(store, f.actor, supplier).await;
    assert!(matches!(
        disposition
            .dispatch_purchase_return(
                f.actor,
                Uuid::new_v4(),
                returned.id,
                "logistics-stale-dispatch",
                &dispatch
            )
            .await,
        Err(DomainError::VersionConflict)
    ));
    let acknowledgment = business_core::b2::AcknowledgePurchaseReturn {
        expected_version: 3,
        acknowledged_date: NaiveDate::from_ymd_opt(2026, 9, 19).unwrap(),
        acknowledgment_note: Some("Supplier receipt verified".into()),
    };
    revoke(store, f.actor, supplier).await;
    assert!(matches!(
        disposition
            .acknowledge_purchase_return(
                f.actor,
                Uuid::new_v4(),
                returned.id,
                "logistics-ack",
                &acknowledgment
            )
            .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    restore(store, f.actor, supplier).await;
    sqlx::query("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2")
        .bind(f.actor)
        .bind(f.brand)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(matches!(
        disposition
            .acknowledge_purchase_return(
                f.actor,
                Uuid::new_v4(),
                returned.id,
                "logistics-brand-denied",
                &acknowledgment
            )
            .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.brand).execute(store.pool()).await.unwrap();
    let acknowledgment_key = super::return_disposition_checks::execute(
        app,
        store,
        f,
        returned.id,
        "purchase_return_acknowledgment_intent",
        serde_json::to_value(&acknowledgment).unwrap(),
    )
    .await;
    let result = disposition
        .acknowledge_purchase_return(
            f.actor,
            Uuid::new_v4(),
            returned.id,
            &acknowledgment_key,
            &acknowledgment,
        )
        .await
        .unwrap();
    assert_eq!(result.status, "supplier_acknowledged");
    assert_eq!(result.version, 4);
    return_reversal_preview_checks::check(app, store, f, false, returned.id, 4).await;
    assert!(
        disposition
            .acknowledge_purchase_return(
                f.actor,
                Uuid::new_v4(),
                returned.id,
                &acknowledgment_key,
                &acknowledgment
            )
            .await
            .unwrap()
            .idempotent_replay
    );
    let quantity: Decimal =
        sqlx::query_scalar("SELECT on_hand_quantity FROM inventory_balances WHERE sku_id=$1")
            .bind(sku)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(quantity, Decimal::ONE);
    let open: Decimal =
        sqlx::query_scalar("SELECT open_amount FROM trade_payables WHERE goods_receipt_id=$1")
            .bind(receipt_id.parse::<Uuid>().unwrap())
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(open, Decimal::from(100));
    let events:i64=sqlx::query_scalar("SELECT count(*) FROM purchase_return_events WHERE purchase_return_id=$1 AND event_type IN ('dispatched','supplier_acknowledged')").bind(returned.id).fetch_one(store.pool()).await.unwrap();
    assert_eq!(events, 2);
    return_reversal_execution_checks::check(app, store, f, false, returned.id, 4).await;
    sales_inspection(app, store, f, "pending").await;
    sales_inspection(app, store, f, "inspected").await;
    sales_inspection(app, store, f, "intervening").await;
}

async fn revoke(store: &PgStore, actor: Uuid, supplier: Uuid) {
    sqlx::query(
        "DELETE FROM business_supplier_scopes WHERE enterprise_user_id=$1 AND supplier_id=$2",
    )
    .bind(actor)
    .bind(supplier)
    .execute(store.pool())
    .await
    .unwrap();
}
async fn restore(store: &PgStore, actor: Uuid, supplier: Uuid) {
    sqlx::query("INSERT INTO business_supplier_scopes(enterprise_user_id,supplier_id,granted_by) VALUES($1,$2,$1)").bind(actor).bind(supplier).execute(store.pool()).await.unwrap();
}

async fn sales_inspection(app: &Router, store: &PgStore, f: &Fixture, scenario: &str) {
    let sku = Uuid::new_v4();
    sqlx::query("INSERT INTO business_skus(id,product_id,code,name) SELECT $1,product_id,$3,'Sales return fixture' FROM business_skus WHERE id=$2").bind(sku).bind(f.sku).bind(format!("SRET-{sku}")).execute(store.pool()).await.unwrap();
    let (status,opening)=call(app,f.actor,"POST","/v1/agent-drafts/inventory-openings",json!({"legalEntityId":f.legal_entity,"businessDate":"2026-09-19","currency":"CNY","lines":[{"warehouseId":f.warehouse,"skuId":sku,"quantity":"2","unitCost":"50"}]})).await;
    assert_eq!(status, StatusCode::OK, "{opening}");
    stock_reversal_checks::confirm(
        app,
        f.actor,
        "stock/inventory_opening",
        opening["id"].as_str().unwrap(),
    )
    .await;
    let (status,order)=call(app,f.actor,"POST","/v1/agent-drafts/sales-orders",json!({"legalEntityId":f.legal_entity,"businessUnitId":f.business_unit,"customerId":f.customer,"currency":"CNY","orderDate":"2026-09-19","lines":[{"skuId":sku,"warehouseId":f.warehouse,"unitOfMeasureId":f.uom,"quantity":"2","unitPrice":"100","discountAmount":"0","taxRate":"0"}]})).await;
    assert_eq!(status, StatusCode::OK, "{order}");
    let id = order["id"].as_str().unwrap();
    stock_reversal_checks::confirm(app, f.actor, "sales-orders", id).await;
    let (_, detail) = call(
        app,
        f.actor,
        "GET",
        &format!("/v1/agent-documents/sales-orders/{id}"),
        Value::Null,
    )
    .await;
    let (status,shipment)=call(app,f.actor,"POST","/v1/agent-drafts/shipments",json!({"salesOrderId":id,"warehouseId":f.warehouse,"shipmentDate":"2026-09-19","lines":[{"salesOrderLineId":detail["lines"][0]["id"],"quantity":"2"}]})).await;
    assert_eq!(status, StatusCode::OK, "{shipment}");
    let id = shipment["id"].as_str().unwrap();
    stock_reversal_checks::confirm(app, f.actor, "stock/shipment", id).await;
    let line: Uuid = sqlx::query_scalar("SELECT id FROM shipment_lines WHERE shipment_id=$1")
        .bind(id.parse::<Uuid>().unwrap())
        .fetch_one(store.pool())
        .await
        .unwrap();
    let service = business_core::b2::ReturnService::new(store.clone(), "SR".into(), "PR".into());
    let input=serde_json::from_value(json!({"sourceId":id,"returnDate":"2026-09-19","reasonCode":"partial defect","lines":[{"sourceLineId":line,"quantity":"1"}]})).unwrap();
    let returned = service
        .create_sales_return(
            f.actor,
            Uuid::new_v4(),
            &format!("inspection-create-{scenario}"),
            &input,
        )
        .await
        .unwrap();
    super::return_confirmation_checks::confirm(app, store, f, true, returned.id).await;
    if scenario == "pending" {
        return_reversal_execution_checks::check(app, store, f, true, returned.id, 2).await;
        return;
    }
    let stored: i64 = sqlx::query_scalar("SELECT version FROM sales_returns WHERE id=$1")
        .bind(returned.id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(stored, 2);
    let disposition = business_core::b2::ReturnDispositionService::new(store.clone());
    let preview = disposition
        .sales_inspection(f.actor, returned.id)
        .await
        .unwrap();
    let input:business_core::b2::InspectSalesReturn=serde_json::from_value(json!({"expectedVersion":preview.version,"inspectionDate":"2026-09-19","inspectionNote":"Half accepted, half scrapped","lines":[{"returnLineId":preview.lines[0].return_line_id,"acceptedQuantity":"0.5","scrapQuantity":"0.5"}]})).unwrap();
    if scenario == "inspected" {
        disposition
            .inspect_sales_return(
                f.actor,
                Uuid::new_v4(),
                returned.id,
                "inspection-clean",
                &input,
            )
            .await
            .unwrap();
        return_reversal_execution_checks::check(app, store, f, true, returned.id, 3).await;
        return;
    }

    sqlx::query("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2")
        .bind(f.actor)
        .bind(f.brand)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(matches!(
        disposition.sales_inspection(f.actor, returned.id).await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    assert!(matches!(
        disposition
            .inspect_sales_return(
                f.actor,
                Uuid::new_v4(),
                returned.id,
                "inspection-brand-denied",
                &input
            )
            .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.brand).execute(store.pool()).await.unwrap();
    super::return_disposition_checks::execute(
        app,
        store,
        f,
        returned.id,
        "sales_return_inspection_intent",
        serde_json::to_value(&input).unwrap(),
    )
    .await;
    assert!(matches!(
        disposition
            .inspect_sales_return(
                f.actor,
                Uuid::new_v4(),
                returned.id,
                "inspection-repeat",
                &input
            )
            .await,
        Err(DomainError::VersionConflict)
    ));
    let balance:(Decimal,Decimal,Decimal)=sqlx::query_as("SELECT on_hand_quantity,quarantined_quantity,inventory_value FROM inventory_balances WHERE sku_id=$1").bind(sku).fetch_one(store.pool()).await.unwrap();
    assert_eq!(
        balance,
        (Decimal::new(75, 2), Decimal::ZERO, Decimal::new(375, 1))
    );
    let open: Decimal =
        sqlx::query_scalar("SELECT open_amount FROM trade_receivables WHERE shipment_id=$1")
            .bind(id.parse::<Uuid>().unwrap())
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(open, Decimal::from(100));
    let (status, blocked) = call(
        app,
        f.actor,
        "POST",
        &format!(
            "/v1/agent-return-reversal-previews/sales_return/{}",
            returned.id
        ),
        json!({"expectedVersion":3,"reversalDate":"2026-09-21","reason":"核实纠错"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{blocked}");
    assert!(
        blocked
            .to_string()
            .contains("intervening inventory movements"),
        "{blocked}"
    );
}
