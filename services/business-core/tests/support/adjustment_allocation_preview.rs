use super::{batch, Fixture};
use business_core::b4::{model::VersionCommand, AdjustmentService};
use chrono::NaiveDate;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;
async fn counts(pool: &PgPool) -> (i64, i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM operational_adjustment_previews),(SELECT count(*) FROM business_core_audit_events),(SELECT count(*) FROM business_command_idempotency),(SELECT count(*) FROM profit_facts)").fetch_one(pool).await.unwrap()
}
pub async fn verify(pool: &PgPool, service: &AdjustmentService, f: &Fixture) {
    let orders:Vec<Uuid>=sqlx::query_scalar("SELECT sales_order_id FROM order_profit_current WHERE legal_entity_id=$1 AND net_revenue>0 ORDER BY sales_order_id LIMIT 2").bind(f.legal_entity).fetch_all(pool).await.unwrap();
    assert_eq!(orders.len(), 2);
    let input = batch(
        f,
        NaiveDate::from_ymd_opt(2026, 8, 21).unwrap(),
        "allocated_operating_expense",
        "10.01",
        "net_revenue",
        orders.clone(),
    );
    let draft = service
        .create(f.actor, Uuid::new_v4(), "pure-adjustment-draft", &input)
        .await
        .unwrap();
    let before = counts(pool).await;
    let v = VersionCommand {
        expected_version: 1,
    };
    let preview = service
        .allocation_preview(f.actor, draft.id, &v)
        .await
        .unwrap();
    assert_eq!(
        service
            .allocation_preview(f.actor, draft.id, &v)
            .await
            .unwrap(),
        preview
    );
    assert_eq!(counts(pool).await, before);
    assert_eq!(preview["preview"]["batch"]["status"], "draft");
    assert_eq!(preview["preview"]["batch"]["version"], 1);
    assert_eq!(
        preview["preview"]["totalAmount"]
            .as_str()
            .unwrap()
            .parse::<rust_decimal::Decimal>()
            .unwrap(),
        "10.01".parse::<rust_decimal::Decimal>().unwrap()
    );
    assert_eq!(
        preview["preview"]["allocatedAmount"]
            .as_str()
            .unwrap()
            .parse::<rust_decimal::Decimal>()
            .unwrap(),
        "10.01".parse::<rust_decimal::Decimal>().unwrap()
    );
    assert_eq!(preview["preview"]["targets"].as_array().unwrap().len(), 2);
    assert!(preview["preview"]["lines"][0]["amount"].is_string());
    let mut tx = pool.begin().await.unwrap();
    assert!(service
        .allocation_preview_on(&mut tx, f.actor, draft.id, &v)
        .await
        .is_err());
    tx.rollback().await.unwrap();
    assert!(service
        .allocation_preview(
            f.actor,
            draft.id,
            &VersionCommand {
                expected_version: 99
            }
        )
        .await
        .is_err());
    sqlx::query(
        "DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2",
    )
    .bind(f.actor)
    .bind(f.customer)
    .execute(pool)
    .await
    .unwrap();
    assert!(service
        .allocation_preview(f.actor, draft.id, &v)
        .await
        .is_err());
    sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.customer).execute(pool).await.unwrap();
    sqlx::query(
        "UPDATE sales_orders SET business_note='pure preview target version change' WHERE id=$1",
    )
    .bind(orders[0])
    .execute(pool)
    .await
    .unwrap();
    let changed = service
        .allocation_preview(f.actor, draft.id, &v)
        .await
        .unwrap();
    assert_ne!(changed["previewHash"], preview["previewHash"]);
    assert_eq!(
        changed["preview"]["allocations"],
        preview["preview"]["allocations"]
    );
    let persisted = service
        .preview(
            f.actor,
            Uuid::new_v4(),
            draft.id,
            "persisted-adjustment-preview",
            &v,
        )
        .await
        .unwrap();
    assert_eq!(
        persisted.allocations["lines"],
        changed["preview"]["allocations"]
    );
    assert_eq!(persisted.batch_version, 2);
    let next = service
        .allocation_preview(
            f.actor,
            draft.id,
            &VersionCommand {
                expected_version: 2,
            },
        )
        .await
        .unwrap();
    assert_eq!(next["preview"]["batch"]["status"], "previewed");
    assert_ne!(next["previewHash"], changed["previewHash"]);
    let stored: Value =
        sqlx::query_scalar("SELECT payload FROM operational_adjustment_previews WHERE id=$1")
            .bind(persisted.preview_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(stored["lines"], next["preview"]["allocations"]);
}
