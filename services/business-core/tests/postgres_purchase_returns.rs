#[path = "support/b3_fixture.rs"]
mod fixture;
use business_core::{
    b2::{
        model::{DecimalString, VersionCommand as B2VersionCommand},
        AcknowledgePurchaseReturn, CreateReturn, DispatchPurchaseReturn, ReturnDispositionService,
        ReturnService,
    },
    b3::{
        model::{
            CreateGoodsReceipt, CreatePurchaseOrder, GoodsReceiptLineInput, PurchaseOrderLineInput,
            VersionCommand,
        },
        PayablesService, PurchasingService, ReceivingService,
    },
    PgStore,
};
use chrono::NaiveDate;
use fixture::*;
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, Row};
use std::str::FromStr;
use uuid::Uuid;
struct Fixture {
    actor: Uuid,
    legal_entity: Uuid,
    business_unit: Uuid,
    warehouse: Uuid,
    supplier: Uuid,
    uom: Uuid,
    sku: Uuid,
}

#[tokio::test]
async fn purchase_return_inventory_payable_and_logistics() {
    let Ok(url) = std::env::var("BUSINESS_CORE_PURCHASE_RETURNS_TEST_DATABASE_URL") else {
        eprintln!("skipping: BUSINESS_CORE_PURCHASE_RETURNS_TEST_DATABASE_URL is not set");
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
    let purchasing = PurchasingService::new(store.clone(), "PO".into(), 30);
    let receiving =
        ReceivingService::new(store.clone(), purchasing.clone(), "GR".into(), "AP".into());
    let returns = ReturnService::new(store.clone(), "SR".into(), "PR".into());
    let payables = PayablesService::new(store.clone(), "PAY".into());
    let disposition = ReturnDispositionService::new(store);
    let order = create_order(&purchasing, &f, date, "pr-order-create", "10", "100").await;
    purchasing
        .confirm_order(
            f.actor,
            Uuid::new_v4(),
            order.id,
            "pr-order-confirm",
            &version(1),
        )
        .await
        .unwrap();
    let line = sqlx::query_scalar("SELECT id FROM purchase_order_lines WHERE purchase_order_id=$1")
        .bind(order.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let receipt = create_receipt(
        &receiving,
        &f,
        date,
        order.id,
        line,
        "10",
        "pr-receipt-create",
    )
    .await;
    receiving
        .confirm_receipt(
            f.actor,
            Uuid::new_v4(),
            receipt.id,
            "pr-receipt-confirm",
            &version(1),
        )
        .await
        .unwrap();
    let option = returns
        .purchase_options(f.actor)
        .await
        .unwrap()
        .items
        .remove(0);
    let input: CreateReturn = serde_json::from_value(json!({"sourceId": receipt.id, "returnDate": date, "reasonCode": "supplier_return", "lines": [{"sourceLineId": option.source_line_id, "quantity": "2"}]})).unwrap();
    sqlx::query("DELETE FROM business_supplier_scopes WHERE enterprise_user_id=$1")
        .bind(f.actor)
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        returns
            .create_purchase_return(f.actor, Uuid::new_v4(), "pr-denied-create", &input)
            .await,
        Err(business_core::b2::DomainError::NotFoundOrForbidden)
    ));
    sqlx::query("INSERT INTO business_supplier_scopes(enterprise_user_id,supplier_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.supplier).execute(&pool).await.unwrap();
    let ret = returns
        .create_purchase_return(f.actor, Uuid::new_v4(), "pr-create", &input)
        .await
        .unwrap();
    assert_eq!(
        returns.purchase_options(f.actor).await.unwrap().items[0]
            .returnable_quantity
            .0,
        decimal("8")
    );
    let too_many: CreateReturn = serde_json::from_value(json!({"sourceId": receipt.id, "returnDate": date, "reasonCode": "supplier_return", "lines": [{"sourceLineId": option.source_line_id, "quantity": "9"}]})).unwrap();
    assert!(returns
        .create_purchase_return(f.actor, Uuid::new_v4(), "pr-over-return", &too_many)
        .await
        .is_err());
    let confirmed = returns
        .confirm_purchase_return(
            f.actor,
            Uuid::new_v4(),
            ret.id,
            "pr-confirm",
            &b2_version(1),
        )
        .await
        .unwrap();
    assert!(
        returns
            .confirm_purchase_return(
                f.actor,
                Uuid::new_v4(),
                ret.id,
                "pr-confirm",
                &b2_version(1)
            )
            .await
            .unwrap()
            .idempotent_replay
    );
    let stored: i64 = sqlx::query_scalar("SELECT version FROM purchase_returns WHERE id=$1")
        .bind(ret.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, confirmed.version);
    assert_balance(&pool, &f, "8", "800", "100").await;
    let before: (Decimal, Decimal) = sqlx::query_as(
        "SELECT original_amount,open_amount FROM trade_payables WHERE goods_receipt_id=$1",
    )
    .bind(receipt.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(before, (decimal("800"), decimal("800")));
    let early_ack = AcknowledgePurchaseReturn {
        expected_version: confirmed.version,
        acknowledged_date: date,
        acknowledgment_note: None,
    };
    assert!(disposition
        .acknowledge_purchase_return(f.actor, Uuid::new_v4(), ret.id, "pr-early-ack", &early_ack)
        .await
        .is_err());
    let dispatch = DispatchPurchaseReturn {
        expected_version: confirmed.version,
        dispatch_date: date,
        carrier: "Test carrier".into(),
        tracking_number: "TEST-ONLY".into(),
    };
    let dispatched = disposition
        .dispatch_purchase_return(f.actor, Uuid::new_v4(), ret.id, "pr-dispatch", &dispatch)
        .await
        .unwrap();
    assert!(
        disposition
            .dispatch_purchase_return(f.actor, Uuid::new_v4(), ret.id, "pr-dispatch", &dispatch)
            .await
            .unwrap()
            .idempotent_replay
    );
    let ack = AcknowledgePurchaseReturn {
        expected_version: dispatched.version,
        ..early_ack
    };
    disposition
        .acknowledge_purchase_return(f.actor, Uuid::new_v4(), ret.id, "pr-acknowledge", &ack)
        .await
        .unwrap();
    assert!(
        disposition
            .acknowledge_purchase_return(f.actor, Uuid::new_v4(), ret.id, "pr-acknowledge", &ack)
            .await
            .unwrap()
            .idempotent_replay
    );
    assert_balance(&pool, &f, "8", "800", "100").await;
    let after: (Decimal, Decimal) = sqlx::query_as(
        "SELECT original_amount,open_amount FROM trade_payables WHERE goods_receipt_id=$1",
    )
    .bind(receipt.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(before, after);
    let state: String =
        sqlx::query_scalar("SELECT logistics_status FROM purchase_returns WHERE id=$1")
            .bind(ret.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state, "supplier_acknowledged");
    // Settled payables must reject a return atomically, preserving stock and draft.
    let payment_input = serde_json::from_value(json!({"legalEntityId": f.legal_entity, "supplierId": f.supplier, "currency": "CNY", "paymentDate": date, "amount": "800", "paymentMethod": "bank_transfer"})).unwrap();
    let payment = payables
        .create_payment(f.actor, Uuid::new_v4(), "pr-payment-create", &payment_input)
        .await
        .unwrap();
    let paid = payables
        .confirm_payment(
            f.actor,
            Uuid::new_v4(),
            payment.id,
            "pr-payment-confirm",
            &version(1),
        )
        .await
        .unwrap();
    let payable_id: Uuid =
        sqlx::query_scalar("SELECT id FROM trade_payables WHERE goods_receipt_id=$1")
            .bind(receipt.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let allocation = serde_json::from_value(json!({"expectedPaymentVersion": paid.version, "allocations": [{"payableId": payable_id, "amount": "800"}]})).unwrap();
    payables
        .apply_payment(
            f.actor,
            Uuid::new_v4(),
            payment.id,
            "pr-payment-apply",
            &allocation,
        )
        .await
        .unwrap();
    let blocked = returns
        .create_purchase_return(f.actor, Uuid::new_v4(), "pr-settled-draft", &input)
        .await
        .unwrap();
    assert!(matches!(
        returns
            .confirm_purchase_return(
                f.actor,
                Uuid::new_v4(),
                blocked.id,
                "pr-settled-confirm",
                &b2_version(1)
            )
            .await,
        Err(business_core::b2::DomainError::PayableAlreadySettled)
    ));
    assert_balance(&pool, &f, "8", "800", "100").await;
    let pending: (String, i64) =
        sqlx::query_as("SELECT status,version FROM purchase_returns WHERE id=$1")
            .bind(blocked.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(pending, ("draft".into(), 1));
    returns
        .cancel_purchase_return(
            f.actor,
            Uuid::new_v4(),
            blocked.id,
            "pr-cancel-draft",
            &b2_version(1),
        )
        .await
        .unwrap();
    assert_eq!(
        returns.purchase_options(f.actor).await.unwrap().items[0]
            .returnable_quantity
            .0,
        decimal("8")
    );
}
