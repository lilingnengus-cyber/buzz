#[path = "support/b2_fixture.rs"]
mod fixture;
use business_core::b2::{
    CreateReturn, InspectSalesReturn, ReturnDispositionService, ReturnService,
};
use business_core::{
    b2::{
        model::{
            CreateInventoryOpening, CreateSalesOrder, CreateShipment, DecimalString,
            InventoryOpeningLineInput, SalesOrderLineInput, ShipmentLineInput, VersionCommand,
        },
        InventoryService, SalesService, SettlementService,
    },
    PgStore,
};
use chrono::NaiveDate;
use fixture::*;
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

struct Fixture {
    actor: Uuid,
    legal_entity: Uuid,
    business_unit: Uuid,
    warehouse: Uuid,
    customer: Uuid,
    brand: Uuid,
    uom: Uuid,
    sku: Uuid,
}

#[tokio::test]
async fn sales_return_quarantine_inspection_and_replay() {
    let Ok(url) = std::env::var("BUSINESS_CORE_RETURNS_TEST_DATABASE_URL") else {
        eprintln!("skipping: BUSINESS_CORE_RETURNS_TEST_DATABASE_URL is not set");
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let f = seed(&pool).await;
    let date = NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
    let sales = SalesService::new(store.clone(), "SO".into(), "SHP".into(), 30);
    let inventory = InventoryService::new(store.clone(), "OPEN".into(), "AR".into());
    let returns = ReturnService::new(store.clone(), "SR".into(), "PR".into());
    let settlement = SettlementService::new(store.clone(), "RCPT".into());
    let disposition = ReturnDispositionService::new(store);
    let opening = inventory
        .create_opening(
            f.actor,
            Uuid::new_v4(),
            "return-opening-create",
            &CreateInventoryOpening {
                legal_entity_id: f.legal_entity,
                business_date: date,
                currency: "CNY".into(),
                lines: vec![InventoryOpeningLineInput {
                    warehouse_id: f.warehouse,
                    sku_id: f.sku,
                    quantity: dec(20),
                    unit_cost: dec(5),
                }],
            },
        )
        .await
        .unwrap();
    inventory
        .post_opening(
            f.actor,
            Uuid::new_v4(),
            opening.id,
            "return-opening-post",
            &version(1),
        )
        .await
        .unwrap();
    let order = create_order(&sales, &f, date, "return-order-create").await;
    sales
        .confirm_order(
            f.actor,
            Uuid::new_v4(),
            order.id,
            "return-order-confirm",
            &version(1),
        )
        .await
        .unwrap();
    let line = sqlx::query_scalar("SELECT id FROM sales_order_lines WHERE sales_order_id=$1")
        .bind(order.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let shipment = sales
        .create_shipment(
            f.actor,
            Uuid::new_v4(),
            "return-shipment-create",
            &CreateShipment {
                sales_order_id: order.id,
                warehouse_id: f.warehouse,
                shipment_date: date,
                lines: vec![ShipmentLineInput {
                    sales_order_line_id: line,
                    quantity: dec(8),
                }],
            },
        )
        .await
        .unwrap();
    inventory
        .confirm_shipment(
            f.actor,
            Uuid::new_v4(),
            shipment.id,
            "return-shipment-confirm",
            &version(1),
        )
        .await
        .unwrap();
    let options = returns.sales_options(f.actor).await.unwrap();
    assert_eq!(options.items[0].returnable_quantity.0, Decimal::from(8));
    let input: CreateReturn = serde_json::from_value(json!({
        "sourceId": shipment.id, "returnDate": date, "reasonCode": "customer_return",
        "lines": [{"sourceLineId": options.items[0].source_line_id, "quantity": "2"}]
    }))
    .unwrap();
    let ret = returns
        .create_sales_return(f.actor, Uuid::new_v4(), "sales-return-create", &input)
        .await
        .unwrap();
    assert_eq!(
        returns.sales_options(f.actor).await.unwrap().items[0]
            .returnable_quantity
            .0,
        Decimal::from(6)
    );
    let excessive: CreateReturn = serde_json::from_value(json!({"sourceId": shipment.id, "returnDate": date, "reasonCode": "customer_return", "lines": [{"sourceLineId": options.items[0].source_line_id, "quantity": "7"}]})).unwrap();
    assert!(returns
        .create_sales_return(
            f.actor,
            Uuid::new_v4(),
            "sales-return-excessive",
            &excessive
        )
        .await
        .is_err());
    let confirmed = returns
        .confirm_sales_return(
            f.actor,
            Uuid::new_v4(),
            ret.id,
            "sales-return-confirm",
            &version(1),
        )
        .await
        .unwrap();
    assert!(
        returns
            .confirm_sales_return(
                f.actor,
                Uuid::new_v4(),
                ret.id,
                "sales-return-confirm",
                &version(1)
            )
            .await
            .unwrap()
            .idempotent_replay
    );
    let balance: (Decimal, Decimal, Decimal) = sqlx::query_as("SELECT on_hand_quantity,quarantined_quantity,inventory_value FROM inventory_balances WHERE sku_id=$1").bind(f.sku).fetch_one(&pool).await.unwrap();
    assert_eq!(
        balance,
        (Decimal::from(14), Decimal::from(2), Decimal::from(70))
    );
    let receivable: (Decimal, Decimal) = sqlx::query_as(
        "SELECT original_amount,open_amount FROM trade_receivables WHERE shipment_id=$1",
    )
    .bind(shipment.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(receivable, (Decimal::from(600), Decimal::from(600)));
    let view = disposition.sales_inspection(f.actor, ret.id).await.unwrap();
    assert_eq!(view.inspection_status, "pending");
    assert_eq!(view.version, confirmed.version);
    // Reject an incomplete disposition without consuming inventory or the version.
    let invalid: InspectSalesReturn = serde_json::from_value(json!({
        "expectedVersion": view.version, "inspectionDate": date,
        "lines": [{"returnLineId": view.lines[0].return_line_id, "acceptedQuantity": "1", "scrapQuantity": "0"}]
    })).unwrap();
    assert!(disposition
        .inspect_sales_return(
            f.actor,
            Uuid::new_v4(),
            ret.id,
            "return-invalid-inspection",
            &invalid
        )
        .await
        .is_err());
    assert_eq!(
        disposition
            .sales_inspection(f.actor, ret.id)
            .await
            .unwrap()
            .version,
        view.version
    );
    let inspection: InspectSalesReturn = serde_json::from_value(json!({
        "expectedVersion": confirmed.version, "inspectionDate": date,
        "lines": [{"returnLineId": view.lines[0].return_line_id, "acceptedQuantity": "1", "scrapQuantity": "1"}]
    })).unwrap();
    disposition
        .inspect_sales_return(
            f.actor,
            Uuid::new_v4(),
            ret.id,
            "return-inspection",
            &inspection,
        )
        .await
        .unwrap();
    assert!(
        disposition
            .inspect_sales_return(
                f.actor,
                Uuid::new_v4(),
                ret.id,
                "return-inspection",
                &inspection
            )
            .await
            .unwrap()
            .idempotent_replay
    );
    assert!(disposition
        .inspect_sales_return(
            f.actor,
            Uuid::new_v4(),
            ret.id,
            "return-inspection-duplicate",
            &inspection
        )
        .await
        .is_err());
    let balance: (Decimal, Decimal, Decimal) = sqlx::query_as("SELECT on_hand_quantity,quarantined_quantity,inventory_value FROM inventory_balances WHERE sku_id=$1").bind(f.sku).fetch_one(&pool).await.unwrap();
    assert_eq!(
        balance,
        (Decimal::from(13), Decimal::ZERO, Decimal::from(65))
    );
    let movements: i64 = sqlx::query_scalar("SELECT count(*) FROM inventory_movements WHERE source_id=$1 AND movement_type='sales_return_scrap'").bind(ret.id).fetch_one(&pool).await.unwrap();
    assert_eq!(movements, 1);
    let receipt_input = serde_json::from_value(json!({"legalEntityId": f.legal_entity, "customerId": f.customer, "currency": "CNY", "receiptDate": date, "amount": "600", "paymentMethod": "bank_transfer"})).unwrap();
    let receipt = settlement
        .create_receipt(
            f.actor,
            Uuid::new_v4(),
            "return-cash-create",
            &receipt_input,
        )
        .await
        .unwrap();
    let cash = settlement
        .confirm_receipt(
            f.actor,
            Uuid::new_v4(),
            receipt.id,
            "return-cash-confirm",
            &version(1),
        )
        .await
        .unwrap();
    let receivable_id: Uuid =
        sqlx::query_scalar("SELECT id FROM trade_receivables WHERE shipment_id=$1")
            .bind(shipment.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let allocation = serde_json::from_value(json!({"expectedReceiptVersion": cash.version, "allocations": [{"receivableId": receivable_id, "amount": "600"}]})).unwrap();
    settlement
        .apply_receipt(
            f.actor,
            Uuid::new_v4(),
            receipt.id,
            "return-cash-apply",
            &allocation,
        )
        .await
        .unwrap();
    let blocked = returns
        .create_sales_return(f.actor, Uuid::new_v4(), "return-settled-draft", &input)
        .await
        .unwrap();
    assert!(matches!(
        returns
            .confirm_sales_return(
                f.actor,
                Uuid::new_v4(),
                blocked.id,
                "return-settled-confirm",
                &version(1)
            )
            .await,
        Err(business_core::b2::DomainError::ReceivableAlreadySettled)
    ));
    let unchanged: (Decimal, Decimal, Decimal) = sqlx::query_as("SELECT on_hand_quantity,quarantined_quantity,inventory_value FROM inventory_balances WHERE sku_id=$1").bind(f.sku).fetch_one(&pool).await.unwrap();
    assert_eq!(unchanged, balance);
    let pending: (String, i64) =
        sqlx::query_as("SELECT status,version FROM sales_returns WHERE id=$1")
            .bind(blocked.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(pending, ("draft".into(), 1));
    returns
        .cancel_sales_return(
            f.actor,
            Uuid::new_v4(),
            blocked.id,
            "return-cancel-draft",
            &version(1),
        )
        .await
        .unwrap();
    assert_eq!(
        returns.sales_options(f.actor).await.unwrap().items[0]
            .returnable_quantity
            .0,
        Decimal::from(6)
    );
}

async fn create_order(
    sales: &SalesService,
    fixture: &Fixture,
    date: NaiveDate,
    key: &str,
) -> business_core::b2::model::CommandResult {
    sales
        .create_order(
            fixture.actor,
            Uuid::new_v4(),
            key,
            &CreateSalesOrder {
                legal_entity_id: fixture.legal_entity,
                customer_id: fixture.customer,
                salesperson_user_id: None,
                business_unit_id: fixture.business_unit,
                department_id: None,
                brand_id: Some(fixture.brand),
                currency: "CNY".into(),
                order_date: date,
                requested_delivery_date: Some(date),
                payment_terms_days: None,
                customer_reference: None,
                business_note: None,
                lines: vec![SalesOrderLineInput {
                    sku_id: fixture.sku,
                    warehouse_id: Some(fixture.warehouse),
                    unit_of_measure_id: fixture.uom,
                    quantity: dec(8),
                    unit_price: dec(100),
                    discount_amount: dec(0),
                    tax_rate: dec(0),
                    business_unit_id: None,
                    department_id: None,
                    brand_id: Some(fixture.brand),
                }],
            },
        )
        .await
        .unwrap()
}

fn dec(value: i64) -> DecimalString {
    DecimalString(Decimal::from(value))
}

fn version(expected_version: i64) -> VersionCommand {
    VersionCommand {
        expected_version,
        reason_code: None,
    }
}
