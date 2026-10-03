use super::*;

pub(super) async fn check(
    pool: &sqlx::PgPool,
    fixture: &Fixture,
    inventory: &InventoryService,
    date: NaiveDate,
) {
    let counts =
        business_core::b2::InventoryCountService::new(PgStore::new(pool.clone()), "COUNT".into());
    assert_eq!(
        counts.options(fixture.actor).await.unwrap()[0].currency,
        "CNY"
    );
    assert_eq!(
        counts.aging(fixture.actor, 0, 10).await.unwrap()["items"][0]["currency"],
        "CNY"
    );
    let count_input = business_core::b2::CreateInventoryCount {
        legal_entity_id: fixture.legal_entity,
        warehouse_id: fixture.warehouse,
        count_date: date,
        currency: "CNY".into(),
        business_note: None,
        sku_ids: vec![fixture.sku],
    };
    let count = counts
        .create(
            fixture.actor,
            Uuid::new_v4(),
            "audit-count-valid",
            &count_input,
        )
        .await
        .unwrap();
    counts
        .cancel(
            fixture.actor,
            Uuid::new_v4(),
            count.id,
            "audit-count-cancel",
            &version(1),
        )
        .await
        .unwrap();
    // Simulate a legacy/out-of-band currency fact appearing after a count snapshot.
    // This is isolated fixture seeding; the application does not bypass the freeze.
    let pending = counts
        .create(
            fixture.actor,
            Uuid::new_v4(),
            "audit-count-pending",
            &count_input,
        )
        .await
        .unwrap();
    let detail = counts.detail(fixture.actor, pending.id).await.unwrap();
    let submission = serde_json::from_value(serde_json::json!({
        "expectedVersion": pending.version,
        "lines": [{"countLineId": detail.lines[0].id, "actualOnHandQuantity": detail.lines[0].snapshot_on_hand_quantity.0.to_string()}]
    })).unwrap();
    let counted = counts
        .submit(
            fixture.actor,
            Uuid::new_v4(),
            pending.id,
            "audit-count-submit",
            &submission,
        )
        .await
        .unwrap();
    sqlx::query("INSERT INTO inventory_movements(id,legal_entity_id,warehouse_id,sku_id,movement_type,quantity,unit_cost,total_cost,currency,source_type,source_id,business_date,created_by_user_id,trace_id,source_line_id) VALUES($1,$2,$3,$4,'inventory_count_adjustment',1,1,1,'USD','inventory_count',$5,$6,$7,$8,$9)")
        .bind(Uuid::new_v4()).bind(fixture.legal_entity).bind(fixture.warehouse).bind(fixture.sku).bind(Uuid::new_v4()).bind(date).bind(fixture.actor).bind(Uuid::new_v4()).bind(detail.lines[0].id).execute(pool).await.unwrap();
    let before_post: (Decimal, Decimal) = sqlx::query_as(
        "SELECT on_hand_quantity,inventory_value FROM inventory_balances WHERE sku_id=$1",
    )
    .bind(fixture.sku)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(matches!(
        counts
            .post(
                fixture.actor,
                Uuid::new_v4(),
                pending.id,
                "audit-count-post-mixed",
                &version(counted.version)
            )
            .await,
        Err(DomainError::Invalid(_))
    ));
    let after_post: (Decimal, Decimal) = sqlx::query_as(
        "SELECT on_hand_quantity,inventory_value FROM inventory_balances WHERE sku_id=$1",
    )
    .bind(fixture.sku)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(before_post, after_post);
    assert_eq!(
        counts
            .detail(fixture.actor, pending.id)
            .await
            .unwrap()
            .status,
        "counted"
    );
    counts
        .cancel(
            fixture.actor,
            Uuid::new_v4(),
            pending.id,
            "audit-count-pending-cancel",
            &version(counted.version),
        )
        .await
        .unwrap();
    // Existing ledgers can contain mixed currencies; the read model must not invent a denomination.
    let mixed = inventory
        .create_opening(
            fixture.actor,
            Uuid::new_v4(),
            "audit-mixed-currency-create",
            &CreateInventoryOpening {
                legal_entity_id: fixture.legal_entity,
                business_date: date,
                currency: "USD".into(),
                lines: vec![InventoryOpeningLineInput {
                    warehouse_id: fixture.warehouse,
                    sku_id: fixture.sku,
                    quantity: dec(1),
                    unit_cost: dec(1),
                }],
            },
        )
        .await
        .unwrap();
    inventory
        .post_opening(
            fixture.actor,
            Uuid::new_v4(),
            mixed.id,
            "audit-mixed-currency-post",
            &version(1),
        )
        .await
        .unwrap();
    let balance = inventory
        .balances(fixture.actor, Some(fixture.sku), 10)
        .await
        .unwrap();
    assert!(balance[0].currency.is_none());
    assert!(balance[0].currency_conflict);
    assert!(counts.options(fixture.actor).await.unwrap().is_empty());
    assert!(counts.aging(fixture.actor, 0, 10).await.unwrap()["items"][0]["currency"].is_null());
    assert!(matches!(
        counts
            .create(
                fixture.actor,
                Uuid::new_v4(),
                "audit-count-mixed",
                &count_input
            )
            .await,
        Err(DomainError::Invalid(_))
    ));
    assert_eq!(
        counts
            .turnover(fixture.actor, "2026-08", "CNY")
            .await
            .unwrap()["endingInventoryValue"],
        "0"
    );

    let turnover = counts
        .turnover(fixture.actor, "2026-08", "CNY")
        .await
        .unwrap();
    assert_eq!(turnover["excludedCurrencyBalances"], 1);
    assert!(turnover["turnoverRate"].is_null());
}
