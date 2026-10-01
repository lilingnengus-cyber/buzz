use super::*;

pub(super) async fn check(sales: &SalesService, pool: &sqlx::PgPool, f: &Fixture, date: NaiveDate) {
    let order = create_order(sales, f, date, "edit-fixture-create").await;
    let options = sales.draft_options(f.actor, order.id).await.unwrap();
    assert_eq!(options["canUpdate"], true);
    let draft = &options["draft"];
    assert_eq!(draft["version"], 1);
    assert_eq!(draft["legalEntityId"], f.legal_entity.to_string());
    let mut input: business_core::b2::model::ReplaceSalesOrderDraft = serde_json::from_value(serde_json::json!({
        "expectedVersion":1, "customerId":draft["customerId"], "businessUnitId":draft["businessUnitId"],
        "departmentId":draft["departmentId"], "brandId":draft["brandId"], "currency":"USD",
        "orderDate":draft["orderDate"], "requestedDeliveryDate":draft["requestedDeliveryDate"],
        "paymentTermsDays":45, "customerReference":"EDIT-REF", "businessNote":"Edited note", "lines":draft["lines"]
    })).unwrap();
    input.lines[0].tax_rate = DecimalString(Decimal::new(13, 2));
    input.lines[0].unit_price = DecimalString(Decimal::new(120, 0));
    let saved = sales
        .replace_order_draft(
            f.actor,
            Uuid::new_v4(),
            order.id,
            "edit-fixture-save",
            &input,
        )
        .await
        .unwrap();
    assert_eq!(saved.version, 2);
    let edited = sales.draft_options(f.actor, order.id).await.unwrap();
    assert_eq!(edited["draft"]["currency"], "USD");
    assert_eq!(edited["draft"]["paymentTermsDays"], 45);
    assert_eq!(edited["draft"]["customerReference"], "EDIT-REF");
    assert_eq!(edited["draft"]["lines"][0]["brandId"], f.brand.to_string());
    assert_eq!(
        edited["draft"]["lines"][0]["taxRate"]
            .as_str()
            .unwrap()
            .parse::<Decimal>()
            .unwrap(),
        Decimal::new(13, 2)
    );
    assert!(matches!(
        sales
            .replace_order_draft(
                f.actor,
                Uuid::new_v4(),
                order.id,
                "edit-fixture-stale",
                &input
            )
            .await,
        Err(DomainError::VersionConflict)
    ));
    sqlx::query("DELETE FROM business_warehouse_scopes WHERE enterprise_user_id=$1")
        .bind(f.actor)
        .execute(pool)
        .await
        .unwrap();
    assert!(matches!(
        sales.draft_options(f.actor, order.id).await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    sqlx::query("INSERT INTO business_warehouse_scopes(enterprise_user_id,warehouse_id,granted_by) VALUES($1,$2,$1)")
        .bind(f.actor)
        .bind(f.warehouse)
        .execute(pool)
        .await
        .unwrap();
    sales
        .delete_order_draft(f.actor, Uuid::new_v4(), order.id, "edit-fixture-delete", 2)
        .await
        .unwrap();
    assert!(matches!(
        sales.draft_options(f.actor, order.id).await,
        Err(DomainError::NotFoundOrForbidden)
    ));
}
