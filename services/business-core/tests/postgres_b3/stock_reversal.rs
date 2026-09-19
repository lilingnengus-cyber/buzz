use super::*;

pub(super) async fn concurrent_inventory_receipts(
    purchasing: &PurchasingService,
    receiving: &ReceivingService,
    f: &Fixture,
    date: NaiveDate,
    pool: &sqlx::PgPool,
) {
    let before = sqlx::query(
        "SELECT on_hand_quantity,inventory_value FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3",
    )
    .bind(f.legal_entity)
    .bind(f.warehouse)
    .bind(f.sku)
    .fetch_one(pool)
    .await
    .unwrap();
    let a = create_order(
        purchasing,
        f,
        date,
        "b3-cost-race-order-create-0001",
        "10",
        "100",
    )
    .await;
    let b = create_order(
        purchasing,
        f,
        date,
        "b3-cost-race-order-create-0002",
        "20",
        "120",
    )
    .await;
    purchasing
        .confirm_order(
            f.actor,
            Uuid::new_v4(),
            a.id,
            "b3-cost-race-order-confirm-0001",
            &version(1),
        )
        .await
        .unwrap();
    purchasing
        .confirm_order(
            f.actor,
            Uuid::new_v4(),
            b.id,
            "b3-cost-race-order-confirm-0002",
            &version(1),
        )
        .await
        .unwrap();
    let line_a: Uuid =
        sqlx::query_scalar("SELECT id FROM purchase_order_lines WHERE purchase_order_id=$1")
            .bind(a.id)
            .fetch_one(pool)
            .await
            .unwrap();
    let line_b: Uuid =
        sqlx::query_scalar("SELECT id FROM purchase_order_lines WHERE purchase_order_id=$1")
            .bind(b.id)
            .fetch_one(pool)
            .await
            .unwrap();
    let receipt_a = create_receipt(
        receiving,
        f,
        date,
        a.id,
        line_a,
        "10",
        "b3-cost-race-receipt-create-0001",
    )
    .await;
    let receipt_b = create_receipt(
        receiving,
        f,
        date,
        b.id,
        line_b,
        "20",
        "b3-cost-race-receipt-create-0002",
    )
    .await;
    let left_version = version(1);
    let right_version = version(1);
    let left = receiving.confirm_receipt(
        f.actor,
        Uuid::new_v4(),
        receipt_a.id,
        "b3-cost-race-receipt-confirm-0001",
        &left_version,
    );
    let right = receiving.confirm_receipt(
        f.actor,
        Uuid::new_v4(),
        receipt_b.id,
        "b3-cost-race-receipt-confirm-0002",
        &right_version,
    );
    let (left, right) = tokio::join!(left, right);
    assert!(left.is_ok() && right.is_ok());
    let after = sqlx::query(
        "SELECT on_hand_quantity,inventory_value,average_unit_cost FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3",
    )
    .bind(f.legal_entity)
    .bind(f.warehouse)
    .bind(f.sku)
    .fetch_one(pool)
    .await
    .unwrap();
    let expected_quantity = before.get::<Decimal, _>("on_hand_quantity") + decimal("30");
    let expected_value = before.get::<Decimal, _>("inventory_value") + decimal("3400");
    assert_eq!(
        after.get::<Decimal, _>("on_hand_quantity"),
        expected_quantity
    );
    assert_eq!(after.get::<Decimal, _>("inventory_value"), expected_value);
    assert_eq!(
        after.get::<Decimal, _>("average_unit_cost"),
        (expected_value / expected_quantity).round_dp(6)
    );
}

pub(super) async fn reversible_receipt(
    purchasing: &PurchasingService,
    receiving: &ReceivingService,
    f: &Fixture,
    date: NaiveDate,
    pool: &sqlx::PgPool,
) {
    let before: Decimal = sqlx::query_scalar(
        "SELECT on_hand_quantity FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3",
    )
    .bind(f.legal_entity)
    .bind(f.warehouse)
    .bind(f.sku)
    .fetch_one(pool)
    .await
    .unwrap();
    let order = create_order(
        purchasing,
        f,
        date,
        "b3-reversible-order-create-0001",
        "1",
        "130",
    )
    .await;
    purchasing
        .confirm_order(
            f.actor,
            Uuid::new_v4(),
            order.id,
            "b3-reversible-order-confirm-0001",
            &version(1),
        )
        .await
        .unwrap();
    let line: Uuid =
        sqlx::query_scalar("SELECT id FROM purchase_order_lines WHERE purchase_order_id=$1")
            .bind(order.id)
            .fetch_one(pool)
            .await
            .unwrap();
    let receipt = create_receipt(
        receiving,
        f,
        date,
        order.id,
        line,
        "1",
        "b3-reversible-receipt-create-0001",
    )
    .await;
    receiving
        .confirm_receipt(
            f.actor,
            Uuid::new_v4(),
            receipt.id,
            "b3-reversible-receipt-confirm-0001",
            &version(1),
        )
        .await
        .unwrap();
    for column in ["reserved_quantity", "quarantined_quantity"] {
        sqlx::query("UPDATE inventory_balances SET reserved_quantity=CASE WHEN $4 THEN on_hand_quantity ELSE 0 END,quarantined_quantity=CASE WHEN $4 THEN 0 ELSE on_hand_quantity END WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3")
            .bind(f.legal_entity)
            .bind(f.warehouse)
            .bind(f.sku)
            .bind(column == "reserved_quantity")
            .execute(pool)
            .await
            .unwrap();
        let blocked = receiving
            .reverse_receipt(
                f.actor,
                Uuid::new_v4(),
                receipt.id,
                &format!("reverse-{column}-blocked"),
                &version(2),
            )
            .await;
        assert!(
            matches!(blocked, Err(DomainError::Invalid(ref message)) if message.contains("reserved or quarantined")),
            "unexpected result: {blocked:?}"
        );
        let row = sqlx::query("SELECT status,version FROM goods_receipts WHERE id=$1")
            .bind(receipt.id)
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(row.get::<String, _>("status"), "confirmed");
        assert_eq!(row.get::<i64, _>("version"), 2);
        sqlx::query("UPDATE inventory_balances SET reserved_quantity=0,quarantined_quantity=0 WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3")
            .bind(f.legal_entity)
            .bind(f.warehouse)
            .bind(f.sku)
            .execute(pool)
            .await
            .unwrap();
    }
    receiving
        .reverse_receipt(
            f.actor,
            Uuid::new_v4(),
            receipt.id,
            "b3-reversible-receipt-reverse-0001",
            &version(2),
        )
        .await
        .unwrap();
    let after: Decimal = sqlx::query_scalar(
        "SELECT on_hand_quantity FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3",
    )
    .bind(f.legal_entity)
    .bind(f.warehouse)
    .bind(f.sku)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(before, after);
}

pub(super) async fn concurrent_over_receipt(
    purchasing: &PurchasingService,
    receiving: &ReceivingService,
    f: &Fixture,
    date: NaiveDate,
    pool: &sqlx::PgPool,
) {
    let order = create_order(
        purchasing,
        f,
        date,
        "b3-race-order-create-0001",
        "10",
        "120",
    )
    .await;
    purchasing
        .confirm_order(
            f.actor,
            Uuid::new_v4(),
            order.id,
            "b3-race-order-confirm-0001",
            &version(1),
        )
        .await
        .unwrap();
    let line: Uuid =
        sqlx::query_scalar("SELECT id FROM purchase_order_lines WHERE purchase_order_id=$1")
            .bind(order.id)
            .fetch_one(pool)
            .await
            .unwrap();
    let left_input = CreateGoodsReceipt {
        purchase_order_id: order.id,
        warehouse_id: f.warehouse,
        receipt_date: date,
        lines: vec![GoodsReceiptLineInput {
            purchase_order_line_id: line,
            quantity: dec("8"),
        }],
    };
    let right_input = left_input.clone();
    let left = receiving.create_receipt(
        f.actor,
        Uuid::new_v4(),
        "b3-race-receipt-create-0001",
        &left_input,
    );
    let right = receiving.create_receipt(
        f.actor,
        Uuid::new_v4(),
        "b3-race-receipt-create-0002",
        &right_input,
    );
    let (left, right) = tokio::join!(left, right);
    let receipt = match (left, right) {
        (Ok(receipt), Err(DomainError::OverReceipt))
        | (Err(DomainError::OverReceipt), Ok(receipt)) => receipt,
        outcome => panic!("expected one draft allocation to win: {outcome:?}"),
    };
    receiving
        .confirm_receipt(
            f.actor,
            Uuid::new_v4(),
            receipt.id,
            "b3-race-receipt-confirm-0001",
            &version(1),
        )
        .await
        .unwrap();
    let received: Decimal =
        sqlx::query_scalar("SELECT received_quantity FROM purchase_order_lines WHERE id=$1")
            .bind(line)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(received, decimal("8"));
}

pub(super) async fn reversal_after_inventory_lock(
    purchasing: &PurchasingService,
    receiving: &ReceivingService,
    inventory: &InventoryService,
    f: &Fixture,
    date: NaiveDate,
    pool: &sqlx::PgPool,
) {
    for earlier_timestamp in [false, true] {
        for opening in [false, true] {
            let id = if opening {
                let item = inventory
                    .create_opening(
                        f.actor,
                        Uuid::new_v4(),
                        &format!("locked-opening-create-{earlier_timestamp}"),
                        &CreateInventoryOpening {
                            legal_entity_id: f.legal_entity,
                            business_date: date,
                            currency: "CNY".into(),
                            lines: vec![InventoryOpeningLineInput {
                                warehouse_id: f.warehouse,
                                sku_id: f.sku,
                                quantity: dec("1"),
                                unit_cost: dec("100"),
                            }],
                        },
                    )
                    .await
                    .unwrap();
                inventory
                    .post_opening(
                        f.actor,
                        Uuid::new_v4(),
                        item.id,
                        &format!("locked-opening-post-{earlier_timestamp}"),
                        &b2_version(1),
                    )
                    .await
                    .unwrap();
                // Quarantine is protected by a domain rejection, not a database constraint error.
                sqlx::query("UPDATE inventory_balances SET quarantined_quantity=on_hand_quantity WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3")
                .bind(f.legal_entity).bind(f.warehouse).bind(f.sku).execute(pool).await.unwrap();
                let blocked = inventory
                    .reverse_opening(
                        f.actor,
                        Uuid::new_v4(),
                        item.id,
                        &format!("quarantined-opening-reverse-{earlier_timestamp}"),
                        &b2_version(2),
                    )
                    .await;
                assert!(
                    matches!(blocked, Err(DomainError::Invalid(ref message)) if message.contains("reserved or quarantined")),
                    "unexpected result: {blocked:?}"
                );
                sqlx::query("UPDATE inventory_balances SET quarantined_quantity=0 WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3")
                .bind(f.legal_entity).bind(f.warehouse).bind(f.sku).execute(pool).await.unwrap();
                item.id
            } else {
                let order = create_order(
                    purchasing,
                    f,
                    date,
                    &format!("locked-receipt-order-{earlier_timestamp}"),
                    "1",
                    "100",
                )
                .await;
                purchasing
                    .confirm_order(
                        f.actor,
                        Uuid::new_v4(),
                        order.id,
                        &format!("locked-receipt-order-confirm-{earlier_timestamp}"),
                        &version(1),
                    )
                    .await
                    .unwrap();
                let line = sqlx::query_scalar(
                    "SELECT id FROM purchase_order_lines WHERE purchase_order_id=$1",
                )
                .bind(order.id)
                .fetch_one(pool)
                .await
                .unwrap();
                let item = create_receipt(
                    receiving,
                    f,
                    date,
                    order.id,
                    line,
                    "1",
                    &format!("locked-receipt-create-{earlier_timestamp}"),
                )
                .await;
                receiving
                    .confirm_receipt(
                        f.actor,
                        Uuid::new_v4(),
                        item.id,
                        &format!("locked-receipt-confirm-{earlier_timestamp}"),
                        &version(1),
                    )
                    .await
                    .unwrap();
                item.id
            };
            let mut tx = pool.begin().await.unwrap();
            let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            let before:Decimal=sqlx::query_scalar("SELECT on_hand_quantity FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3 FOR UPDATE")
            .bind(f.legal_entity).bind(f.warehouse).bind(f.sku).fetch_one(&mut *tx).await.unwrap();
            let inventory = inventory.clone();
            let receiving = receiving.clone();
            let actor = f.actor;
            let task = tokio::spawn(async move {
                if opening {
                    inventory
                        .reverse_opening(
                            actor,
                            Uuid::new_v4(),
                            id,
                            &format!("locked-opening-reverse-{earlier_timestamp}"),
                            &b2_version(2),
                        )
                        .await
                        .map(|_| ())
                } else {
                    receiving
                        .reverse_receipt(
                            actor,
                            Uuid::new_v4(),
                            id,
                            &format!("locked-receipt-reverse-{earlier_timestamp}"),
                            &version(2),
                        )
                        .await
                        .map(|_| ())
                }
            });
            tokio::time::timeout(std::time::Duration::from_secs(5),async {
            loop {
                let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))")
                    .bind(blocker).fetch_one(pool).await.unwrap();
                if waiting {break;}
                assert!(!task.is_finished(),"reversal must wait for the inventory row lock");
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.unwrap();
            // Simulate an inventory writer committing after the reversal's initial reads.
            let movement = Uuid::new_v4();
            sqlx::query("INSERT INTO inventory_movements(id,legal_entity_id,warehouse_id,sku_id,movement_type,quantity,unit_cost,total_cost,currency,source_type,source_id,source_line_id,business_date,created_by_user_id,trace_id,posted_at) VALUES($1,$2,$3,$4,'opening_balance',1,1,1,'CNY','test_concurrent_opening',$5,$6,$7,$8,$9,$10)")
            .bind(movement).bind(f.legal_entity).bind(f.warehouse).bind(f.sku).bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(date).bind(f.actor).bind(Uuid::new_v4()).bind(if earlier_timestamp {chrono::Utc::now()-chrono::Duration::days(1)}else{chrono::Utc::now()}).execute(&mut *tx).await.unwrap();
            sqlx::query("UPDATE inventory_balances SET on_hand_quantity=on_hand_quantity+1,inventory_value=inventory_value+1,average_unit_cost=round((inventory_value+1)/(on_hand_quantity+1),6),last_movement_id=$4 WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3")
            .bind(f.legal_entity).bind(f.warehouse).bind(f.sku).bind(movement).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap();
            if opening {
                assert!(
                    matches!(result,Err(DomainError::Invalid(ref message)) if message.contains("subsequent inventory movements")),
                    "unexpected result: {result:?}"
                );
            } else {
                assert!(
                    matches!(result, Err(DomainError::SubsequentInventoryMovementsExist)),
                    "unexpected result: {result:?}"
                );
            }
            let after:Decimal=sqlx::query_scalar("SELECT on_hand_quantity FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3")
            .bind(f.legal_entity).bind(f.warehouse).bind(f.sku).fetch_one(pool).await.unwrap();
            assert_eq!(after, before + Decimal::ONE);
            let reversals:i64=sqlx::query_scalar("SELECT count(*) FROM inventory_movements WHERE source_id=$1 AND reversal_of_movement_id IS NOT NULL").bind(id).fetch_one(pool).await.unwrap();
            assert_eq!(reversals, 0);
        }
    }
}
