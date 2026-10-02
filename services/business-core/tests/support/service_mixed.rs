use business_core::{
    b2::{model::VersionCommand, InventoryService, SalesService},
    service_delivery::{AcceptanceInput, ProjectInput, ServiceDelivery},
    PgStore,
};
use chrono::NaiveDate;
use sqlx::{PgPool, Row};
use uuid::Uuid;
pub async fn check(pool: &PgPool, ids: (Uuid, Uuid, Uuid, Uuid, Uuid, Uuid, Uuid)) {
    let (actor, legal, unit, customer, service_sku, uom, category) = ids;
    let role: Uuid =
        sqlx::query_scalar("SELECT role_id FROM business_user_roles WHERE enterprise_user_id=$1")
            .bind(actor)
            .fetch_one(pool)
            .await
            .unwrap();
    for permission in [
        "inventory_opening:create",
        "inventory_opening:post",
        "shipment:create",
        "customer_receipt:create",
        "customer_receipt:confirm",
        "receivable_allocation:create",
        "receivable_allocation:reverse",
        "shipment:confirm",
        "shipment:reverse",
        "sales_order:cancel",
    ] {
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(role).bind(permission).execute(pool).await.unwrap();
    }
    let warehouse = Uuid::new_v4();
    let product = Uuid::new_v4();
    let sku = Uuid::new_v4();
    sqlx::query("INSERT INTO business_warehouses(id,legal_entity_id,business_unit_id,code,name) VALUES($1,$2,$3,'MIX_WH','仓库')").bind(warehouse).bind(legal).bind(unit).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_warehouse_scopes(enterprise_user_id,warehouse_id,granted_by) VALUES($1,$2,$1)").bind(actor).bind(warehouse).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_products(id,code,name,category_id,base_uom_id) VALUES($1,'DEVICE','设备',$2,$3)").bind(product).bind(category).bind(uom).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_skus(id,product_id,code,name) VALUES($1,$2,'DEVICE_SKU','设备')",
    )
    .bind(sku)
    .bind(product)
    .execute(pool)
    .await
    .unwrap();
    let inventory = InventoryService::new(PgStore::new(pool.clone()), "OPEN".into(), "AR".into());
    let sales = SalesService::new(PgStore::new(pool.clone()), "SO".into(), "SHP".into(), 30);
    let services = ServiceDelivery::new(PgStore::new(pool.clone()));
    let version = |v| VersionCommand {
        expected_version: v,
        reason_code: None,
    };
    let opening=inventory.create_opening(actor,Uuid::new_v4(),"mixed-opening-create",&serde_json::from_value(serde_json::json!({"legalEntityId":legal,"businessDate":"2026-10-01","currency":"CNY","lines":[{"warehouseId":warehouse,"skuId":sku,"quantity":"5","unitCost":"2"}]})).unwrap()).await.unwrap();
    inventory
        .post_opening(
            actor,
            Uuid::new_v4(),
            opening.id,
            "mixed-opening-post",
            &version(1),
        )
        .await
        .unwrap();
    // Reject attempts to give a service SKU physical inventory.
    assert!(sqlx::query(
        "INSERT INTO inventory_balances(legal_entity_id,warehouse_id,sku_id) VALUES($1,$2,$3)"
    )
    .bind(legal)
    .bind(warehouse)
    .bind(service_sku)
    .execute(pool)
    .await
    .is_err());
    let input=serde_json::from_value(serde_json::json!({"legalEntityId":legal,"customerId":customer,"businessUnitId":unit,"currency":"CNY","orderDate":"2026-10-01","lines":[{"skuId":service_sku,"warehouseId":null,"unitOfMeasureId":uom,"quantity":"1","unitPrice":"100"},{"skuId":sku,"warehouseId":warehouse,"unitOfMeasureId":uom,"quantity":"1","unitPrice":"10"}]})).unwrap();
    let order = sales
        .create_order(actor, Uuid::new_v4(), "mixed-order-create", &input)
        .await
        .unwrap();
    assert_eq!(
        sales
            .confirmation_preview(actor, order.id)
            .await
            .unwrap()
            .lines
            .len(),
        1
    );
    sales
        .confirm_order(
            actor,
            Uuid::new_v4(),
            order.id,
            "mixed-order-confirm",
            &version(1),
        )
        .await
        .unwrap();
    let rows=sqlx::query("SELECT id,service_kind FROM sales_order_lines WHERE sales_order_id=$1 ORDER BY line_number").bind(order.id).fetch_all(pool).await.unwrap();
    let service_line: Uuid = rows[0].get("id");
    let goods_line: Uuid = rows[1].get("id");
    let project_input:ProjectInput=serde_json::from_value(serde_json::json!({"title":"设备配套软件","legalEntityId":legal,"businessUnitId":unit,"customerId":customer,"ownerUserId":actor,"contactName":"张经理","serviceKind":"software_service","salesOrderLineId":service_line,"startsOn":"2026-10-01","endsOn":"2027-09-30","status":"acceptance","description":""})).unwrap();
    let project = services
        .save_project(
            actor,
            Uuid::new_v4(),
            None,
            "mixed-project-create",
            &project_input,
        )
        .await
        .unwrap();
    let project_id = serde_json::from_value(project["id"].clone()).unwrap();
    services
        .accept(
            actor,
            Uuid::new_v4(),
            project_id,
            "mixed-accept",
            &AcceptanceInput {
                accepted_on: NaiveDate::from_ymd_opt(2026, 10, 2).unwrap(),
                customer_reviewer: "张经理".into(),
                result: "passed".into(),
                note: "功能通过".into(),
                evidence_url: "".into(),
                expected_version: 1,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        sales
            .get_order(actor, order.id)
            .await
            .unwrap()
            .lifecycle_status,
        "confirmed"
    );
    assert_eq!(
        sales
            .get_order(actor, order.id)
            .await
            .unwrap()
            .fulfillment_status,
        "partially_fulfilled"
    );
    let shipment=sales.create_shipment(actor,Uuid::new_v4(),"mixed-shipment-create",&serde_json::from_value(serde_json::json!({"salesOrderId":order.id,"warehouseId":warehouse,"shipmentDate":"2026-10-02","lines":[{"salesOrderLineId":goods_line,"quantity":"1"}]})).unwrap()).await.unwrap();
    inventory
        .confirm_shipment(
            actor,
            Uuid::new_v4(),
            shipment.id,
            "mixed-shipment-confirm",
            &version(1),
        )
        .await
        .unwrap();
    assert_eq!(
        sales
            .get_order(actor, order.id)
            .await
            .unwrap()
            .lifecycle_status,
        "completed"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM trade_receivables WHERE sales_order_id=$1"
        )
        .bind(order.id)
        .fetch_one(pool)
        .await
        .unwrap(),
        2
    );
    let progress = sales.order_detail(actor, order.id).await.unwrap();
    assert_eq!(progress["progress"]["goods"][0]["complete"], true);
    assert_eq!(progress["progress"]["services"][0]["complete"], true);
    assert_eq!(progress["progress"]["payment"]["amount"], "110.000000");
    inventory
        .reverse_shipment(
            actor,
            Uuid::new_v4(),
            shipment.id,
            "mixed-shipment-reverse",
            &version(2),
        )
        .await
        .unwrap();
    assert_eq!(
        sales
            .get_order(actor, order.id)
            .await
            .unwrap()
            .fulfillment_status,
        "partially_fulfilled"
    );
    assert_eq!(
        services.detail(actor, project_id).await.unwrap()["receivable"]["openAmount"],
        "100.000000"
    );
    let settlement =
        business_core::b2::SettlementService::new(PgStore::new(pool.clone()), "RCPT".into());
    let goods = settlement
        .receivables_for_source(actor, None, 1, Some("goods"))
        .await
        .unwrap();
    assert_eq!(goods.len(), 1);
    assert!(goods[0].shipment_id.is_some());
    let service = settlement
        .receivables_for_source(actor, None, 1, Some("service"))
        .await
        .unwrap();
    assert_eq!(service.len(), 1);
    assert!(service[0].service_project_id.is_some());
    assert!(settlement
        .receivables_for_source(actor, None, 1, Some("unknown"))
        .await
        .is_err());
    assert!(settlement
        .receivables_for_source(Uuid::new_v4(), None, 1, Some("service"))
        .await
        .is_err());

    let progress = sales.order_detail(actor, order.id).await.unwrap();
    assert_eq!(progress["progress"]["goods"][0]["complete"], false);
    assert_eq!(progress["progress"]["goods"][0]["remaining"], "1.000000");
    assert_eq!(progress["progress"]["services"][0]["complete"], true);
    assert_eq!(progress["progress"]["payment"]["amount"], "100.000000");
    let receivable_id: Uuid =
        sqlx::query_scalar("SELECT id FROM trade_receivables WHERE service_project_id=$1")
            .bind(project_id)
            .fetch_one(pool)
            .await
            .unwrap();
    let receipt = settlement.create_receipt(actor,Uuid::new_v4(),"progress-receipt", &serde_json::from_value(serde_json::json!({"legalEntityId":legal,"customerId":customer,"currency":"CNY","receiptDate":"2026-10-03","amount":"80","paymentMethod":"bank_transfer"})).unwrap()).await.unwrap();
    settlement
        .confirm_receipt(
            actor,
            Uuid::new_v4(),
            receipt.id,
            "progress-confirm",
            &version(1),
        )
        .await
        .unwrap();
    assert_eq!(
        sales.order_detail(actor, order.id).await.unwrap()["progress"]["payment"]["settled"],
        "0.000000"
    );
    settlement.apply_receipt(actor,Uuid::new_v4(),receipt.id,"progress-apply", &serde_json::from_value(serde_json::json!({"expectedReceiptVersion":2,"allocations":[{"receivableId":receivable_id,"amount":"40"}]})).unwrap()).await.unwrap();
    let progress = sales.order_detail(actor, order.id).await.unwrap();
    assert_eq!(progress["progress"]["payment"]["settled"], "40.000000");
    assert_eq!(progress["progress"]["payment"]["open"], "60.000000");
    let allocation: Uuid =
        sqlx::query_scalar("SELECT id FROM receivable_allocations WHERE receipt_id=$1")
            .bind(receipt.id)
            .fetch_one(pool)
            .await
            .unwrap();
    let ar_version: i64 = sqlx::query_scalar("SELECT version FROM trade_receivables WHERE id=$1")
        .bind(receivable_id)
        .fetch_one(pool)
        .await
        .unwrap();
    settlement.reverse_allocation(actor,Uuid::new_v4(),allocation,"progress-reverse", &serde_json::from_value(serde_json::json!({"expectedReceiptVersion":3,"expectedReceivableVersion":ar_version})).unwrap()).await.unwrap();
    assert_eq!(
        sales.order_detail(actor, order.id).await.unwrap()["progress"]["payment"]["settled"],
        "0.000000"
    );
    sqlx::query(
        "DELETE FROM business_warehouse_scopes WHERE enterprise_user_id=$1 AND warehouse_id=$2",
    )
    .bind(actor)
    .bind(warehouse)
    .execute(pool)
    .await
    .unwrap();
    let scoped = sales.order_detail(actor, order.id).await.unwrap();
    assert!(scoped["progress"]["goods"].is_null());
    assert!(scoped["progress"]["services"].is_array());
    sqlx::query("DELETE FROM business_role_permissions WHERE role_id=$1 AND permission_key IN ('service_delivery:read','receivable:read')").bind(role).execute(pool).await.unwrap();
    let restricted = sales.order_detail(actor, order.id).await.unwrap();
    assert!(restricted["progress"]["goods"].is_null());
    assert!(restricted["progress"]["services"].is_null());
    assert!(restricted["progress"]["payment"].is_null());
    assert!(sales.order_detail(Uuid::new_v4(), order.id).await.is_err());
}
