use super::*;
use business_core::{
    b2::{InventoryService, SalesService},
    b4::{AdjustmentService, ProfitProjectionService},
};
pub async fn source(pool: &PgPool, f: &Fixture) -> Uuid {
    for permission in [
        "profit_adjustment:create",
        "profit_adjustment:preview",
        "profit_adjustment:post",
    ] {
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,$2 FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).bind(permission).execute(pool).await.unwrap();
    }
    let store = PgStore::new(pool.clone());
    let inventory = InventoryService::new(store.clone(), "OPEN".into(), "AR".into());
    let sales = SalesService::new(store.clone(), "SO".into(), "SHP".into(), 30);
    let opening=inventory.create_opening(f.actor,Uuid::new_v4(),"adjustment-source-opening",&serde_json::from_value(json!({"legalEntityId":f.legal_entity,"businessDate":"2026-08-21","currency":"CNY","lines":[{"warehouseId":f.warehouse,"skuId":f.sku,"quantity":"2","unitCost":"30"}]})).unwrap()).await.unwrap();
    inventory
        .post_opening(
            f.actor,
            Uuid::new_v4(),
            opening.id,
            "adjustment-source-opening-post",
            &serde_json::from_value(json!({"expectedVersion":1})).unwrap(),
        )
        .await
        .unwrap();
    let order=sales.create_order(f.actor,Uuid::new_v4(),"adjustment-source-order",&serde_json::from_value(json!({"legalEntityId":f.legal_entity,"customerId":f.customer,"salespersonUserId":f.actor,"businessUnitId":f.business_unit,"brandId":f.brand,"currency":"CNY","orderDate":"2026-08-21","lines":[{"skuId":f.sku,"warehouseId":f.warehouse,"unitOfMeasureId":f.uom,"quantity":"1","unitPrice":"100","discountAmount":"0","taxRate":"0"}]})).unwrap()).await.unwrap();
    sales
        .confirm_order(
            f.actor,
            Uuid::new_v4(),
            order.id,
            "adjustment-source-order-confirm",
            &serde_json::from_value(json!({"expectedVersion":1})).unwrap(),
        )
        .await
        .unwrap();
    let line: Uuid = sqlx::query_scalar("SELECT id FROM sales_order_lines WHERE sales_order_id=$1")
        .bind(order.id)
        .fetch_one(pool)
        .await
        .unwrap();
    let shipment=sales.create_shipment(f.actor,Uuid::new_v4(),"adjustment-source-shipment",&serde_json::from_value(json!({"salesOrderId":order.id,"warehouseId":f.warehouse,"shipmentDate":"2026-08-21","lines":[{"salesOrderLineId":line,"quantity":"1"}]})).unwrap()).await.unwrap();
    inventory
        .confirm_shipment(
            f.actor,
            Uuid::new_v4(),
            shipment.id,
            "adjustment-source-shipment-confirm",
            &serde_json::from_value(json!({"expectedVersion":1})).unwrap(),
        )
        .await
        .unwrap();
    let projection = ProfitProjectionService::new(store);
    let result = projection
        .project_pending(f.actor, Uuid::new_v4(), 100)
        .await
        .unwrap();
    assert_eq!(result["factsProjected"], 2);
    order.id
}
pub async fn draft(pool: &PgPool, f: &Fixture, order: Uuid, key: &str) -> Uuid {
    AdjustmentService::new(PgStore::new(pool.clone()),"ADJ".into(),500).create(f.actor,Uuid::new_v4(),key,&serde_json::from_value(json!({"legalEntityId":f.legal_entity,"currency":"CNY","managementPeriod":"2026-08","lines":[{"metricType":"allocated_operating_expense","amount":"10.01","businessDate":"2026-08-21","allocationBasis":"direct","directSalesOrderId":order,"reasonCode":"TEST"}]})).unwrap()).await.unwrap().id
}
