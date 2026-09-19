use super::{version, Fixture};
use business_core::{
    b2::{CreateInventoryCount, DomainError, InventoryCountOperation as Op, InventoryCountService},
    PgStore,
};
use rust_decimal::Decimal;
use serde_json::json;
use uuid::Uuid;

async fn balance(store: &PgStore, f: &Fixture) -> (Decimal, Decimal, Option<Decimal>) {
    sqlx::query_as("SELECT on_hand_quantity,inventory_value,average_unit_cost FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3").bind(f.legal_entity).bind(f.warehouse).bind(f.sku).fetch_one(store.pool()).await.unwrap()
}
async fn submission(service: &InventoryCountService, f: &Fixture, id: Uuid, actual: &str) -> Op {
    let detail = service.detail(f.actor, id).await.unwrap();
    Op::Submit(serde_json::from_value(json!({"expectedVersion":detail.version,"lines":[{"countLineId":detail.lines[0].id,"actualOnHandQuantity":actual,"surplusUnitCost":"7"}]})).unwrap())
}
pub(super) async fn check(
    store: &PgStore,
    service: &InventoryCountService,
    f: &Fixture,
    input: &CreateInventoryCount,
) {
    let sku = Uuid::new_v4();
    sqlx::query("INSERT INTO business_skus(id,product_id,code,name) SELECT $1,product_id,'COUNT-OPERATIONS','Count operations SKU' FROM business_skus WHERE id=$2").bind(sku).bind(f.sku).execute(store.pool()).await.unwrap();
    sqlx::query(
        "INSERT INTO inventory_balances(legal_entity_id,warehouse_id,sku_id) VALUES($1,$2,$3)",
    )
    .bind(f.legal_entity)
    .bind(f.warehouse)
    .bind(sku)
    .execute(store.pool())
    .await
    .unwrap();
    let isolated = Fixture {
        actor: f.actor,
        legal_entity: f.legal_entity,
        business_unit: f.business_unit,
        warehouse: f.warehouse,
        customer: f.customer,
        brand: f.brand,
        uom: f.uom,
        sku,
    };
    let mut isolated_input = input.clone();
    isolated_input.sku_ids = vec![sku];
    let f = &isolated;
    let input = &isolated_input;
    let initial = balance(store, f).await;
    assert_eq!(initial, (Decimal::ZERO, Decimal::ZERO, None));
    let count = service
        .create(f.actor, Uuid::new_v4(), "count-ops-surplus", input)
        .await
        .unwrap();
    let submit = submission(service, f, count.id, "2").await;
    let snapshot = service
        .operation_preview(f.actor, count.id, &submit)
        .await
        .unwrap();
    assert_eq!(snapshot["retainsFreeze"], true);
    assert_eq!(snapshot["postsInventoryDifferences"], false);
    assert_eq!(
        snapshot["lines"][0]["impact"]["varianceValue"]
            .as_str()
            .unwrap()
            .parse::<Decimal>()
            .unwrap(),
        Decimal::from(14)
    );
    assert_eq!(balance(store, f).await, initial);
    let changed = submission(service, f, count.id, "3").await;
    assert!(matches!(
        service
            .execute_guarded(
                (f.actor, Uuid::new_v4()),
                count.id,
                "count-ops-changed",
                &changed,
                &snapshot
            )
            .await,
        Err(DomainError::StalePreview)
    ));
    let saved = service
        .execute_guarded(
            (f.actor, Uuid::new_v4()),
            count.id,
            "count-ops-submit",
            &submit,
            &snapshot,
        )
        .await
        .unwrap();
    assert_eq!(saved.status, "counted");
    assert_eq!(balance(store, f).await, initial);
    assert!(
        service
            .execute_guarded(
                (f.actor, Uuid::new_v4()),
                count.id,
                "count-ops-submit",
                &submit,
                &snapshot
            )
            .await
            .unwrap()
            .idempotent_replay
    );
    let post = Op::Post(version(2));
    let snapshot = service
        .operation_preview(f.actor, count.id, &post)
        .await
        .unwrap();
    assert_eq!(snapshot["retainsFreeze"], false);
    assert_eq!(snapshot["postsInventoryDifferences"], true);
    sqlx::query(
        "UPDATE inventory_count_lines SET actual_on_hand_quantity=3 WHERE inventory_count_id=$1",
    )
    .bind(count.id)
    .execute(store.pool())
    .await
    .unwrap();
    assert!(matches!(
        service
            .execute_guarded(
                (f.actor, Uuid::new_v4()),
                count.id,
                "count-ops-stale-line",
                &post,
                &snapshot
            )
            .await,
        Err(DomainError::StalePreview)
    ));
    assert_eq!(balance(store, f).await, initial);
    sqlx::query(
        "UPDATE inventory_count_lines SET actual_on_hand_quantity=2 WHERE inventory_count_id=$1",
    )
    .bind(count.id)
    .execute(store.pool())
    .await
    .unwrap();
    let posted = service
        .execute_guarded(
            (f.actor, Uuid::new_v4()),
            count.id,
            "count-ops-post",
            &post,
            &snapshot,
        )
        .await
        .unwrap();
    assert_eq!(posted.version, 3);
    assert_eq!(
        balance(store, f).await,
        (Decimal::from(2), Decimal::from(14), Some(Decimal::from(7)))
    );
    assert!(
        service
            .execute_guarded(
                (f.actor, Uuid::new_v4()),
                count.id,
                "count-ops-post",
                &post,
                &snapshot
            )
            .await
            .unwrap()
            .idempotent_replay
    );
    let cancel = Op::Cancel(business_core::b2::model::VersionCommand {
        expected_version: 3,
        reason_code: Some("cannot cancel posted".into()),
    });
    assert!(service
        .operation_preview(f.actor, count.id, &cancel)
        .await
        .is_err());

    sqlx::query("UPDATE inventory_balances SET reserved_quantity=1 WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3").bind(f.legal_entity).bind(f.warehouse).bind(f.sku).execute(store.pool()).await.unwrap();
    let protected = service
        .create(f.actor, Uuid::new_v4(), "count-ops-protected", input)
        .await
        .unwrap();
    assert!(matches!(
        service
            .operation_preview(
                f.actor,
                protected.id,
                &submission(service, f, protected.id, "0").await
            )
            .await,
        Err(DomainError::Invalid(_))
    ));
    assert!(service
        .operation_preview(f.actor, protected.id, &Op::Cancel(version(1)))
        .await
        .is_err());
    let cancel = Op::Cancel(business_core::b2::model::VersionCommand {
        expected_version: 1,
        reason_code: Some("release count for order fulfillment".into()),
    });
    let snapshot = service
        .operation_preview(f.actor, protected.id, &cancel)
        .await
        .unwrap();
    assert_eq!(snapshot["retainsFreeze"], false);
    assert_eq!(snapshot["postsInventoryDifferences"], false);
    assert!(snapshot["lines"][0]["impact"].is_null());
    let before = balance(store, f).await;
    let cancelled = service
        .execute_guarded(
            (f.actor, Uuid::new_v4()),
            protected.id,
            "count-ops-cancel",
            &cancel,
            &snapshot,
        )
        .await
        .unwrap();
    assert_eq!(cancelled.status, "cancelled");
    assert_eq!(balance(store, f).await, before);
    assert!(
        service
            .execute_guarded(
                (f.actor, Uuid::new_v4()),
                protected.id,
                "count-ops-cancel",
                &cancel,
                &snapshot
            )
            .await
            .unwrap()
            .idempotent_replay
    );
    let reason:String=sqlx::query_scalar("SELECT payload->>'reason' FROM inventory_count_events WHERE inventory_count_id=$1 AND event_type='cancelled'").bind(protected.id).fetch_one(store.pool()).await.unwrap();
    assert_eq!(reason, "release count for order fulfillment");
    sqlx::query("UPDATE inventory_balances SET reserved_quantity=0 WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3").bind(f.legal_entity).bind(f.warehouse).bind(f.sku).execute(store.pool()).await.unwrap();

    let shortage = service
        .create(f.actor, Uuid::new_v4(), "count-ops-shortage", input)
        .await
        .unwrap();
    let submit = submission(service, f, shortage.id, "0").await;
    let snapshot = service
        .operation_preview(f.actor, shortage.id, &submit)
        .await
        .unwrap();
    service
        .execute_guarded(
            (f.actor, Uuid::new_v4()),
            shortage.id,
            "count-ops-shortage-submit",
            &submit,
            &snapshot,
        )
        .await
        .unwrap();
    let snapshot = service
        .operation_preview(f.actor, shortage.id, &post)
        .await
        .unwrap();
    assert_eq!(
        snapshot["lines"][0]["impact"]["varianceValue"]
            .as_str()
            .unwrap()
            .parse::<Decimal>()
            .unwrap(),
        Decimal::from(-14)
    );
    service
        .execute_guarded(
            (f.actor, Uuid::new_v4()),
            shortage.id,
            "count-ops-shortage-post",
            &post,
            &snapshot,
        )
        .await
        .unwrap();
    assert_eq!(balance(store, f).await, initial);
    let totals:(Decimal,Decimal)=sqlx::query_as("SELECT sum(quantity),sum(total_cost) FROM inventory_movements WHERE source_type='inventory_count'").fetch_one(store.pool()).await.unwrap();
    assert_eq!(totals, (Decimal::ZERO, Decimal::ZERO));
}
