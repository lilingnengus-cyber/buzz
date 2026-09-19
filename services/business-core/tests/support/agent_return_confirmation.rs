use super::*;
use sqlx::Row;

fn command(preview: &Value) -> Value {
    json!({"expectedVersion":preview["item"]["version"],"previewHash":preview["previewHash"],"decision":"approve","sourceBuzzEventId":Uuid::new_v4().simple().to_string().repeat(2),"sourceChannelId":"return-confirmation-fixture"})
}
pub(super) async fn confirm(app: &Router, store: &PgStore, f: &Fixture, sales: bool, id: Uuid) {
    let kind = if sales {
        "sales_return"
    } else {
        "purchase_return"
    };
    let preview_path = format!("/v1/agent-approval-previews/returns/{kind}/{id}");
    let approve = format!("/v1/agent-approvals/returns/{kind}/{id}");
    let (status, mut preview) = call(app, f.actor, "GET", &preview_path, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["item"]["version"], 1);
    assert_eq!(
        preview["item"]["amount"]
            .as_str()
            .unwrap()
            .parse::<Decimal>()
            .unwrap(),
        Decimal::from(100)
    );
    assert_eq!(
        preview["item"]["cost"]
            .as_str()
            .unwrap()
            .parse::<Decimal>()
            .unwrap(),
        Decimal::from(if sales { 50 } else { 100 })
    );
    assert_eq!(
        preview["item"]["openAmountAfter"]
            .as_str()
            .unwrap()
            .parse::<Decimal>()
            .unwrap(),
        Decimal::from(100)
    );
    let mut invalid = command(&preview);
    invalid["quantity"] = json!("100");
    assert_eq!(
        call(app, f.actor, "POST", &approve, invalid).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut stale = command(&preview);
    stale["previewHash"] = json!("0".repeat(64));
    assert_eq!(
        call(app, f.actor, "POST", &approve, stale).await.0,
        StatusCode::CONFLICT
    );
    sqlx::query(
        "DELETE FROM business_warehouse_scopes WHERE enterprise_user_id=$1 AND warehouse_id=$2",
    )
    .bind(f.actor)
    .bind(f.warehouse)
    .execute(store.pool())
    .await
    .unwrap();
    assert_eq!(
        call(app, f.actor, "POST", &approve, command(&preview))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO business_warehouse_scopes(enterprise_user_id,warehouse_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.warehouse).execute(store.pool()).await.unwrap();
    if !sales {
        race_after_preview(app, store, f, &approve, &preview).await;
        let (status, next) = call(app, f.actor, "GET", &preview_path, Value::Null).await;
        assert_eq!(status, StatusCode::OK, "{next}");
        assert_ne!(preview["previewHash"], next["previewHash"]);
        preview = next;
    }
    let cmd = command(&preview);
    let (status, result) = call(app, f.actor, "POST", &approve, cmd.clone()).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true, "{result}");
    let replacement = json!({"expectedVersion":preview["item"]["version"].as_i64().unwrap()+1,"expectedSourceVersion":2,"returnDate":"2026-09-19","reasonCode":"quality","lines":[{"sourceLineId":Uuid::new_v4(),"quantity":"1"}]});
    assert_eq!(
        call(
            app,
            f.actor,
            "PUT",
            &format!("/v1/agent-drafts/returns/{kind}/{id}"),
            replacement
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    assert_eq!(
        result["resourceRefs"][0]["bizUri"],
        format!("biz://{}/{id}", kind.replace('_', "-"))
    );
    assert!(!call(app, f.actor, "POST", &approve, cmd)
        .await
        .0
        .is_success());
    let effect = &preview["item"]["lines"][0];
    let row=sqlx::query("SELECT on_hand_quantity,reserved_quantity,quarantined_quantity,inventory_value FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3").bind(f.legal_entity).bind(f.warehouse).bind(effect["skuId"].as_str().unwrap().parse::<Uuid>().unwrap()).fetch_one(store.pool()).await.unwrap();
    for (column, key) in [
        ("on_hand_quantity", "onHandQuantityAfter"),
        ("reserved_quantity", "reservedQuantityAfter"),
        ("quarantined_quantity", "quarantinedQuantityAfter"),
        ("inventory_value", "inventoryValueAfter"),
    ] {
        assert_eq!(
            row.get::<Decimal, _>(column),
            effect[key].as_str().unwrap().parse::<Decimal>().unwrap()
        );
    }
}

async fn race_after_preview(
    app: &Router,
    store: &PgStore,
    f: &Fixture,
    approve: &str,
    preview: &Value,
) {
    // Pause approval persistence after the full preview/hash has been checked.
    // Changing inventory here exercises execution's locked guard, not merely
    // the earlier HTTP preview comparison.
    sqlx::query("CREATE OR REPLACE FUNCTION test_return_approval_gate() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.document_type='purchase_return' THEN PERFORM pg_advisory_xact_lock(123456789); END IF; RETURN NEW; END $$").execute(store.pool()).await.unwrap();
    sqlx::query("CREATE TRIGGER test_return_approval_gate BEFORE INSERT ON business_document_approval_requests FOR EACH ROW EXECUTE FUNCTION test_return_approval_gate()").execute(store.pool()).await.unwrap();
    let mut blocker = store.pool().begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(123456789)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let app = app.clone();
    let actor = f.actor;
    let path = approve.to_owned();
    let cmd = command(preview);
    let task = tokio::spawn(async move { call(&app, actor, "POST", &path, cmd).await });
    tokio::time::timeout(std::time::Duration::from_secs(10),async {
        loop {
            let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)))").bind(pid).fetch_one(store.pool()).await.unwrap();
            if waiting{break;}
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("approval reached its verified post-preview wait");
    let sku = preview["item"]["lines"][0]["skuId"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    sqlx::query("UPDATE inventory_balances SET reserved_quantity=reserved_quantity+0.25 WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3").bind(f.legal_entity).bind(f.warehouse).bind(sku).execute(store.pool()).await.unwrap();
    blocker.commit().await.unwrap();
    let (status, result) = task.await.unwrap();
    assert_eq!(status, StatusCode::CONFLICT, "{result}");
    assert_eq!(result["code"], "approval_execution_failed");
    sqlx::query("DROP TRIGGER test_return_approval_gate ON business_document_approval_requests")
        .execute(store.pool())
        .await
        .unwrap();
}
