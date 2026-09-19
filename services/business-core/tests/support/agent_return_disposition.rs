use super::*;
use sqlx::Row;

fn command(preview: &Value) -> Value {
    json!({"expectedVersion":1,"previewHash":preview["previewHash"],"decision":"approve","sourceBuzzEventId":Uuid::new_v4().simple().to_string().repeat(2),"sourceChannelId":"return-disposition-fixture"})
}
pub(super) async fn execute(
    app: &Router,
    store: &PgStore,
    f: &Fixture,
    id: Uuid,
    kind: &str,
    command_input: Value,
) -> String {
    let path = format!("/v1/agent-return-disposition-intents/{kind}");
    let input = json!({"sourceDocumentId":id,"command":command_input});
    let mut unknown = input.clone();
    unknown["command"]["execute"] = json!(true);
    assert_eq!(
        call(app, f.actor, "POST", &path, unknown).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut stale = input.clone();
    stale["command"]["expectedVersion"] = json!(100);
    assert_eq!(
        call(app, f.actor, "POST", &path, stale).await.0,
        StatusCode::CONFLICT
    );
    let mut invalid_parameters = input.clone();
    match kind {
        "sales_return_inspection_intent" => {
            invalid_parameters["command"]["lines"][0]["acceptedQuantity"] = json!("999")
        }
        "purchase_return_dispatch_intent" => invalid_parameters["command"]["carrier"] = json!("  "),
        _ => invalid_parameters["command"]["acknowledgedDate"] = json!("2026-09-18"),
    }
    assert_eq!(
        call(app, f.actor, "POST", &path, invalid_parameters)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let key = format!("disposition-prepare-{kind}");
    let (status, mut prepared) = call_key(app, f.actor, "POST", &path, input.clone(), &key).await;
    assert_eq!(status, StatusCode::OK, "{prepared}");
    let (_, replay) = call_key(app, f.actor, "POST", &path, input.clone(), &key).await;
    assert_eq!(replay["item"]["id"], prepared["item"]["id"]);
    let mut different = input.clone();
    let date_key = match kind {
        "sales_return_inspection_intent" => "inspectionDate",
        "purchase_return_dispatch_intent" => "dispatchDate",
        _ => "acknowledgedDate",
    };
    different["command"][date_key] = json!("2026-09-20");
    assert_eq!(
        call_key(app, f.actor, "POST", &path, different, &key)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (_, rejected) = call(app, f.actor, "POST", &path, input.clone()).await;
    let mut rejection = command(&rejected);
    rejection["decision"] = json!("reject");
    let (status, outcome) = call(
        app,
        f.actor,
        "POST",
        &format!(
            "/v1/agent-approvals/return-dispositions/{kind}/{}",
            rejected["item"]["id"].as_str().unwrap()
        ),
        rejection,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["executed"], false);
    assert_eq!(outcome["status"], "rejected");
    let intent = prepared["item"]["id"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    assert!(sqlx::query(
        "UPDATE business_agent_return_disposition_intents SET snapshot='{}'::jsonb WHERE id=$1"
    )
    .bind(intent)
    .execute(store.pool())
    .await
    .is_err());
    let expired = Uuid::new_v4();
    sqlx::query("INSERT INTO business_agent_return_disposition_intents(id,kind,source_document_id,input,snapshot,created_by_user_id,idempotency_key,trace_id,expires_at) SELECT $1,kind,source_document_id,input,snapshot,created_by_user_id,$2,trace_id,now()-interval '1 second' FROM business_agent_return_disposition_intents WHERE id=$3").bind(expired).bind(format!("expired-{expired}")).bind(intent).execute(store.pool()).await.unwrap();
    assert_eq!(
        call(
            app,
            f.actor,
            "GET",
            &format!("/v1/agent-approval-previews/return-dispositions/{kind}/{expired}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let mut approve = format!("/v1/agent-approvals/return-dispositions/{kind}/{intent}");
    let mut invalid = command(&prepared);
    invalid["command"] = json!({});
    assert_eq!(
        call(app, f.actor, "POST", &approve, invalid).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut wrong_hash = command(&prepared);
    wrong_hash["previewHash"] = json!("0".repeat(64));
    assert_eq!(
        call(app, f.actor, "POST", &approve, wrong_hash).await.0,
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
        call(app, f.actor, "POST", &approve, command(&prepared))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO business_warehouse_scopes(enterprise_user_id,warehouse_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.warehouse).execute(store.pool()).await.unwrap();
    if kind == "sales_return_inspection_intent" {
        race(app, store, f, &approve, &prepared).await;
        let (status, next) = call(app, f.actor, "POST", &path, input).await;
        assert_eq!(status, StatusCode::OK, "{next}");
        assert_ne!(prepared["previewHash"], next["previewHash"]);
        prepared = next;
        approve = format!(
            "/v1/agent-approvals/return-dispositions/{kind}/{}",
            prepared["item"]["id"].as_str().unwrap()
        );
    }
    let cmd = command(&prepared);
    let (status, result) = call(app, f.actor, "POST", &approve, cmd.clone()).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true, "{result}");
    let family = if kind.starts_with("sales_") {
        "sales-return"
    } else {
        "purchase-return"
    };
    assert_eq!(
        result["resourceRefs"][0]["bizUri"],
        format!("biz://{family}/{id}")
    );
    assert!(!call(app, f.actor, "POST", &approve, cmd)
        .await
        .0
        .is_success());
    for effect in prepared["document"]["lines"].as_array().unwrap() {
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
    format!(
        "agent-return-disposition:{}",
        prepared["item"]["id"].as_str().unwrap()
    )
}

async fn race(app: &Router, store: &PgStore, f: &Fixture, path: &str, prepared: &Value) {
    sqlx::query("CREATE OR REPLACE FUNCTION test_inspection_gate() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.document_type='sales_return_inspection_intent' THEN PERFORM pg_advisory_xact_lock(76543210); END IF; RETURN NEW; END $$").execute(store.pool()).await.unwrap();
    sqlx::query("CREATE TRIGGER test_inspection_gate BEFORE INSERT ON business_document_approval_requests FOR EACH ROW EXECUTE FUNCTION test_inspection_gate()").execute(store.pool()).await.unwrap();
    let mut blocker = store.pool().begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(76543210)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let cloned = app.clone();
    let actor = f.actor;
    let path = path.to_owned();
    let cmd = command(prepared);
    let task = tokio::spawn(async move { call(&cloned, actor, "POST", &path, cmd).await });
    tokio::time::timeout(std::time::Duration::from_secs(10),async{loop{
        let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)))").bind(pid).fetch_one(store.pool()).await.unwrap();
        if waiting{break;}tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }}).await.expect("inspection confirmation reached post-preview lock wait");
    let sku = prepared["document"]["lines"][0]["skuId"].clone();
    let (status,opening)=call(app,f.actor,"POST","/v1/agent-drafts/inventory-openings",json!({"legalEntityId":f.legal_entity,"businessDate":"2026-09-19","currency":"CNY","lines":[{"warehouseId":f.warehouse,"skuId":sku,"quantity":"0.25","unitCost":"50"}]})).await;
    assert_eq!(status, StatusCode::OK, "{opening}");
    stock_reversal_checks::confirm(
        app,
        f.actor,
        "stock/inventory_opening",
        opening["id"].as_str().unwrap(),
    )
    .await;
    blocker.commit().await.unwrap();
    let (status, result) = task.await.unwrap();
    assert_eq!(status, StatusCode::CONFLICT, "{result}");
    assert_eq!(result["code"], "approval_execution_failed");
    sqlx::query("DROP TRIGGER test_inspection_gate ON business_document_approval_requests")
        .execute(store.pool())
        .await
        .unwrap();
}
