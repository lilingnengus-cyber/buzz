use super::*;

pub(super) async fn check(
    service: &PurchasingService,
    pool: &sqlx::PgPool,
    f: &Fixture,
    date: NaiveDate,
) {
    let draft = create_order(service, f, date, "delete-create-test", "1", "1").await;
    assert!(matches!(
        service
            .delete_order_draft(
                Uuid::new_v4(),
                Uuid::new_v4(),
                draft.id,
                "delete-no-permission",
                1
            )
            .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    assert!(matches!(
        service
            .delete_order_draft(
                f.actor,
                Uuid::new_v4(),
                draft.id,
                "delete-stale-version",
                99
            )
            .await,
        Err(DomainError::VersionConflict)
    ));
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM inventory_movements")
        .fetch_one(pool)
        .await
        .unwrap();
    let result = service
        .delete_order_draft(f.actor, Uuid::new_v4(), draft.id, "delete-success-test", 1)
        .await
        .unwrap();
    assert_eq!(result.status, "deleted");
    assert_eq!(result.version, 2);
    assert!(!result.idempotent_replay);
    let replay = service
        .delete_order_draft(f.actor, Uuid::new_v4(), draft.id, "delete-success-test", 1)
        .await
        .unwrap();
    assert!(replay.idempotent_replay);
    assert!(matches!(
        service
            .delete_order_draft(f.actor, Uuid::new_v4(), draft.id, "delete-new-key-test", 2)
            .await,
        Err(DomainError::Invalid(_))
    ));
    assert!(!service
        .orders(f.actor, None, 200)
        .await
        .unwrap()
        .iter()
        .any(|row| row.id == draft.id));
    assert!(matches!(
        service.get_order(f.actor, draft.id).await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM purchase_order_events WHERE purchase_order_id=$1 AND event_type='draft_deleted'").bind(draft.id).fetch_one(pool).await.unwrap();
    assert_eq!(events, 1);
    let kept: i64 = sqlx::query_scalar("SELECT count(*) FROM purchase_orders WHERE id=$1")
        .bind(draft.id)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(kept, 1);
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM inventory_movements")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(before, after);
    let other = create_order(service, f, date, "delete-other-test", "1", "1").await;
    assert!(matches!(
        service
            .delete_order_draft(f.actor, Uuid::new_v4(), other.id, "delete-success-test", 1)
            .await,
        Err(DomainError::IdempotencyConflict)
    ));
    sqlx::query("UPDATE purchase_orders SET lifecycle_status='confirmed' WHERE id=$1")
        .bind(other.id)
        .execute(pool)
        .await
        .unwrap();
    assert!(matches!(
        service
            .delete_order_draft(
                f.actor,
                Uuid::new_v4(),
                other.id,
                "delete-confirmed-test",
                2
            )
            .await,
        Err(DomainError::Invalid(_))
    ));
    sqlx::query("UPDATE purchase_orders SET lifecycle_status='draft' WHERE id=$1")
        .bind(other.id)
        .execute(pool)
        .await
        .unwrap();
    service
        .delete_order_draft(f.actor, Uuid::new_v4(), other.id, "delete-cleanup-test", 3)
        .await
        .unwrap();
    let delivery = business_core::b3::DeliveryService::new(PgStore::new(pool.clone()));
    assert!(delivery
        .deliveries(f.actor, None, 200)
        .await
        .unwrap()
        .items
        .is_empty());
    assert!(delivery
        .supplier_performance(f.actor, 365)
        .await
        .unwrap()
        .items
        .is_empty());
}
