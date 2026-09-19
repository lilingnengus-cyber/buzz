use super::{version, Fixture};
use business_core::{
    b2::{CreateInventoryCount, DomainError, InventoryCountService, SubmitInventoryCount},
    PgStore,
};
use chrono::NaiveDate;
use serde_json::json;
use uuid::Uuid;

pub(super) async fn check(store: &PgStore, f: &Fixture) {
    sqlx::query(
        "INSERT INTO inventory_balances(legal_entity_id,warehouse_id,sku_id) VALUES($1,$2,$3)",
    )
    .bind(f.legal_entity)
    .bind(f.warehouse)
    .bind(f.sku)
    .execute(store.pool())
    .await
    .unwrap();
    let service = InventoryCountService::new(store.clone(), "CNT".into());
    let input = CreateInventoryCount {
        legal_entity_id: f.legal_entity,
        warehouse_id: f.warehouse,
        count_date: NaiveDate::from_ymd_opt(2026, 8, 21).unwrap(),
        currency: "CNY".into(),
        business_note: None,
        sku_ids: vec![f.sku],
    };
    let first = service
        .create(f.actor, Uuid::new_v4(), "count-create-first", &input)
        .await
        .unwrap();
    let detail = service.detail(f.actor, first.id).await.unwrap();
    let line = json!({"countLineId":detail.lines[0].id,"actualOnHandQuantity":"0"});
    let duplicate: SubmitInventoryCount =
        serde_json::from_value(json!({"expectedVersion":1,"lines":[line.clone(),line.clone()]}))
            .unwrap();
    assert!(matches!(
        service
            .submit(
                f.actor,
                Uuid::new_v4(),
                first.id,
                "count-duplicate-lines",
                &duplicate
            )
            .await,
        Err(DomainError::Invalid(_))
    ));
    let unchanged = service.detail(f.actor, first.id).await.unwrap();
    assert_eq!(unchanged.version, 1);
    assert_eq!(unchanged.status, "counting");
    let submit: SubmitInventoryCount =
        serde_json::from_value(json!({"expectedVersion":1,"lines":[line]})).unwrap();
    let submitted = service
        .submit(
            f.actor,
            Uuid::new_v4(),
            first.id,
            "count-submit-first",
            &submit,
        )
        .await
        .unwrap();
    assert_eq!(submitted.version, 2);
    assert!(
        service
            .submit(
                f.actor,
                Uuid::new_v4(),
                first.id,
                "count-submit-first",
                &submit
            )
            .await
            .unwrap()
            .idempotent_replay
    );
    service
        .post(
            f.actor,
            Uuid::new_v4(),
            first.id,
            "count-post-shared",
            &version(2),
        )
        .await
        .unwrap();
    assert!(
        service
            .post(
                f.actor,
                Uuid::new_v4(),
                first.id,
                "count-post-shared",
                &version(2)
            )
            .await
            .unwrap()
            .idempotent_replay
    );
    let second = service
        .create(f.actor, Uuid::new_v4(), "count-create-second", &input)
        .await
        .unwrap();
    let detail = service.detail(f.actor, second.id).await.unwrap();
    let second_submit:SubmitInventoryCount=serde_json::from_value(json!({"expectedVersion":1,"lines":[{"countLineId":detail.lines[0].id,"actualOnHandQuantity":"0"}]})).unwrap();
    service
        .submit(
            f.actor,
            Uuid::new_v4(),
            second.id,
            "count-submit-second",
            &second_submit,
        )
        .await
        .unwrap();
    assert!(matches!(
        service
            .post(
                f.actor,
                Uuid::new_v4(),
                second.id,
                "count-post-shared",
                &version(2)
            )
            .await,
        Err(DomainError::IdempotencyConflict)
    ));
    assert_eq!(
        service.detail(f.actor, second.id).await.unwrap().status,
        "counted"
    );
    service
        .cancel(
            f.actor,
            Uuid::new_v4(),
            second.id,
            "count-cancel-shared",
            &version(2),
        )
        .await
        .unwrap();
    assert!(
        service
            .cancel(
                f.actor,
                Uuid::new_v4(),
                second.id,
                "count-cancel-shared",
                &version(2)
            )
            .await
            .unwrap()
            .idempotent_replay
    );
    let third = service
        .create(f.actor, Uuid::new_v4(), "count-create-third", &input)
        .await
        .unwrap();
    assert!(matches!(
        service
            .cancel(
                f.actor,
                Uuid::new_v4(),
                third.id,
                "count-cancel-shared",
                &version(2)
            )
            .await,
        Err(DomainError::IdempotencyConflict)
    ));
    assert_eq!(
        service.detail(f.actor, third.id).await.unwrap().status,
        "counting"
    );
    // Same payload and key must not return an unrelated submitted document,
    // even when the new target is otherwise within the actor's data scope.
    assert!(matches!(
        service
            .submit(
                f.actor,
                Uuid::new_v4(),
                third.id,
                "count-submit-first",
                &submit
            )
            .await,
        Err(DomainError::IdempotencyConflict)
    ));
    service
        .cancel(
            f.actor,
            Uuid::new_v4(),
            third.id,
            "count-cancel-third",
            &version(1),
        )
        .await
        .unwrap();
    let movements: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM inventory_movements WHERE source_type='inventory_count'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(movements, 0);
}
