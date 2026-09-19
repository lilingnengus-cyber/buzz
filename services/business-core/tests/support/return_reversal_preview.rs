use super::*;

pub(super) async fn check(
    app: &Router,
    store: &PgStore,
    f: &Fixture,
    sales: bool,
    id: Uuid,
    version: i64,
) {
    let kind = if sales {
        "sales_return"
    } else {
        "purchase_return"
    };
    let path = format!("/v1/agent-return-reversal-previews/{kind}/{id}");
    let input =
        json!({"expectedVersion":version,"reversalDate":"2026-09-21","reason":"核实退货登记有误"});
    let before:Value=sqlx::query_scalar("SELECT jsonb_build_array((SELECT count(*) FROM inventory_movements),(SELECT count(*) FROM business_document_approval_requests),(SELECT count(*) FROM business_agent_return_disposition_intents),(SELECT sum(inventory_value) FROM inventory_balances),(SELECT sum(open_amount) FROM trade_receivables),(SELECT sum(open_amount) FROM trade_payables))").fetch_one(store.pool()).await.unwrap();
    let (status, preview) = call(app, f.actor, "POST", &path, input.clone()).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    let d = &preview["document"];
    assert_eq!(d["source"]["id"], id.to_string());
    assert_eq!(d["source"]["version"], version);
    assert_eq!(d["statusAfter"], "reversed");
    let amount = |key: &str| {
        d["financial"][key]
            .as_str()
            .unwrap()
            .parse::<Decimal>()
            .unwrap()
    };
    assert_eq!(
        amount("openAmountAfter") - amount("openAmountBefore"),
        Decimal::from(100)
    );
    assert_eq!(
        amount("originalAmountAfter") - amount("originalAmountBefore"),
        Decimal::from(100)
    );
    let effect = &d["lines"][0];
    let quantity = |key: &str| effect[key].as_str().unwrap().parse::<Decimal>().unwrap();
    assert_eq!(
        quantity("onHandQuantityAfter") - quantity("onHandQuantityBefore"),
        if sales { -Decimal::ONE } else { Decimal::ONE }
    );
    assert_eq!(
        quantity("quarantinedQuantityAfter") - quantity("quarantinedQuantityBefore"),
        if sales { -Decimal::ONE } else { Decimal::ZERO }
    );
    assert_eq!(d["inverseMovements"].as_array().unwrap().len(), 1);
    let movement = d["inverseMovements"][0]["reversesMovementId"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    let sequence: i64 =
        sqlx::query_scalar("SELECT posting_sequence FROM inventory_movements WHERE id=$1")
            .bind(movement)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert!(sequence > 0);
    let missing_sequence=sqlx::query("INSERT INTO inventory_movements(id,legal_entity_id,warehouse_id,sku_id,movement_type,quantity,unit_cost,total_cost,currency,source_type,source_id,source_line_id,business_date,created_by_user_id,trace_id,posting_sequence) SELECT $1,legal_entity_id,warehouse_id,sku_id,movement_type,quantity,unit_cost,total_cost,currency,source_type,$1,$1,business_date,created_by_user_id,trace_id,NULL FROM inventory_movements WHERE id=$2").bind(Uuid::new_v4()).bind(movement).execute(store.pool()).await;
    let error = missing_sequence.unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.constraint()),
        Some("inventory_movements_new_posting_sequence")
    );

    let (_, again) = call(app, f.actor, "POST", &path, input.clone()).await;
    assert_eq!(again["document"], d.clone());
    for (field, value, expected) in [
        ("expectedVersion", json!(version + 1), StatusCode::CONFLICT),
        ("reversalDate", json!("2020-01-01"), StatusCode::BAD_REQUEST),
        ("reason", json!("  "), StatusCode::BAD_REQUEST),
        ("execute", json!(true), StatusCode::UNPROCESSABLE_ENTITY),
    ] {
        let mut invalid = input.clone();
        invalid[field] = value;
        assert_eq!(call(app, f.actor, "POST", &path, invalid).await.0, expected);
    }
    sqlx::query("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2")
        .bind(f.actor)
        .bind(f.brand)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        call(app, f.actor, "POST", &path, input).await.0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.brand).execute(store.pool()).await.unwrap();
    let after:Value=sqlx::query_scalar("SELECT jsonb_build_array((SELECT count(*) FROM inventory_movements),(SELECT count(*) FROM business_document_approval_requests),(SELECT count(*) FROM business_agent_return_disposition_intents),(SELECT sum(inventory_value) FROM inventory_balances),(SELECT sum(open_amount) FROM trade_receivables),(SELECT sum(open_amount) FROM trade_payables))").fetch_one(store.pool()).await.unwrap();
    assert_eq!(
        before, after,
        "preview cannot write business or approval records"
    );
}
