use super::*;
use business_core::b2::{CreateInventoryCount, InventoryCountService};
fn command(prepared: &Value) -> Value {
    json!({"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve","sourceBuzzEventId":Uuid::new_v4().simple().to_string().repeat(2),"sourceChannelId":"count-operation-test"})
}
async fn state(store: &PgStore, id: Uuid) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('task',to_jsonb(t),'lines',(SELECT jsonb_agg(to_jsonb(l) ORDER BY l.id) FROM inventory_count_lines l WHERE l.inventory_count_id=t.id),'balances',(SELECT jsonb_agg(to_jsonb(b) ORDER BY b.sku_id) FROM inventory_balances b WHERE b.legal_entity_id=t.legal_entity_id AND b.warehouse_id=t.warehouse_id AND b.sku_id IN (SELECT sku_id FROM inventory_count_lines WHERE inventory_count_id=t.id)),'events',(SELECT count(*) FROM inventory_count_events WHERE inventory_count_id=t.id),'movements',(SELECT count(*) FROM inventory_movements WHERE source_type='inventory_count' AND source_id=t.id)) FROM inventory_count_tasks t WHERE id=$1").bind(id).fetch_one(store.pool()).await.unwrap()
}
async fn exercise(
    app: &Router,
    store: &PgStore,
    f: &Fixture,
    id: Uuid,
    kind: &str,
    operation: Value,
) -> Value {
    let before = state(store, id).await;
    let input = json!({"inventoryCountId":id,"operation":operation});
    let prepare = format!("/v1/agent-inventory-count-operation-intents/{kind}");
    let dry = format!("/v1/agent-inventory-count-operation-previews/{kind}");
    let (status, result) = call(app, f.actor, "POST", &dry, input.clone()).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(state(store, id).await, before);
    let mut unknown = input.clone();
    unknown["operation"]["command"]["execute"] = true.into();
    assert!(call(app, f.actor, "POST", &prepare, unknown)
        .await
        .0
        .is_client_error());
    let mut mismatch = input.clone();
    mismatch["operation"]["operation"] = "unexpected".into();
    assert!(call(app, f.actor, "POST", &prepare, mismatch)
        .await
        .0
        .is_client_error());
    let other_kind = if kind == "inventory_count_posting_intent" {
        "inventory_count_cancellation_intent"
    } else {
        "inventory_count_posting_intent"
    };
    assert_eq!(
        call(
            app,
            f.actor,
            "POST",
            &format!("/v1/agent-inventory-count-operation-intents/{other_kind}"),
            input.clone()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let key = format!("count-operation-{kind}-{id}");
    let (status, prepared) = call_key(app, f.actor, "POST", &prepare, input.clone(), &key).await;
    assert_eq!(status, StatusCode::OK, "{prepared}");
    let (_, again) = call_key(app, f.actor, "POST", &prepare, input.clone(), &key).await;
    assert_eq!(again["item"]["id"], prepared["item"]["id"]);
    let mut changed = input.clone();
    if kind == "inventory_count_submission_intent" {
        changed["operation"]["command"]["lines"][0]["actualOnHandQuantity"] = "999".into();
    } else {
        changed["operation"]["command"]["reasonCode"] = "changed confirmation content".into();
    }
    assert_eq!(
        call_key(app, f.actor, "POST", &prepare, changed, &key)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let intent = prepared["item"]["id"].as_str().unwrap();
    let preview = format!("/v1/agent-approval-previews/inventory-count-operations/{kind}/{intent}");
    let approve = format!("/v1/agent-approvals/inventory-count-operations/{kind}/{intent}");
    assert!(sqlx::query(
        "UPDATE business_agent_inventory_count_operation_intents SET snapshot='{}' WHERE id=$1"
    )
    .bind(intent.parse::<Uuid>().unwrap())
    .execute(store.pool())
    .await
    .is_err());
    let expired = Uuid::new_v4();
    sqlx::query("INSERT INTO business_agent_inventory_count_operation_intents(id,kind,inventory_count_id,input,snapshot,created_by_user_id,idempotency_key,expires_at,trace_id) SELECT $2,kind,inventory_count_id,input,snapshot,created_by_user_id,$3,now()-interval '1 minute',trace_id FROM business_agent_inventory_count_operation_intents WHERE id=$1").bind(intent.parse::<Uuid>().unwrap()).bind(expired).bind(expired.to_string()).execute(store.pool()).await.unwrap();
    assert_eq!(
        call(
            app,
            f.actor,
            "GET",
            &format!("/v1/agent-approval-previews/inventory-count-operations/{kind}/{expired}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let mut stale = command(&prepared);
    stale["previewHash"] = "0".repeat(64).into();
    assert_eq!(
        call(app, f.actor, "POST", &approve, stale).await.0,
        StatusCode::CONFLICT
    );
    let mut extra = command(&prepared);
    extra["actualQuantity"] = "999".into();
    assert!(call(app, f.actor, "POST", &approve, extra)
        .await
        .0
        .is_client_error());
    sqlx::query("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2")
        .bind(f.actor)
        .bind(f.brand)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        call(app, f.actor, "GET", &preview, Value::Null).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(app, f.actor, "POST", &approve, command(&prepared))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.brand).execute(store.pool()).await.unwrap();
    let (_, rejected) = call(app, f.actor, "POST", &prepare, input.clone()).await;
    let mut rejection = command(&rejected);
    rejection["decision"] = "reject".into();
    let (status, result) = call(
        app,
        f.actor,
        "POST",
        &format!(
            "/v1/agent-approvals/inventory-count-operations/{kind}/{}",
            rejected["item"]["id"].as_str().unwrap()
        ),
        rejection,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], false);
    assert_eq!(state(store, id).await, before);
    let (_, faulted) = call(app, f.actor, "POST", &prepare, input.clone()).await;
    sqlx::query("CREATE FUNCTION fail_count_operation_execution() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.document_type IN ('inventory_count_submission_intent','inventory_count_posting_intent','inventory_count_cancellation_intent') AND NEW.status='executed' THEN RAISE EXCEPTION 'injected count operation failure'; END IF; RETURN NEW; END $$").execute(store.pool()).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_count_operation_execution AFTER UPDATE ON business_document_approval_requests FOR EACH ROW EXECUTE FUNCTION fail_count_operation_execution()").execute(store.pool()).await.unwrap();
    let (status, result) = call(
        app,
        f.actor,
        "POST",
        &format!(
            "/v1/agent-approvals/inventory-count-operations/{kind}/{}",
            faulted["item"]["id"].as_str().unwrap()
        ),
        command(&faulted),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{result}");
    assert_eq!(
        state(store, id).await,
        before,
        "failed approval must roll back operation and inventory"
    );
    sqlx::query(
        "DROP TRIGGER fail_count_operation_execution ON business_document_approval_requests",
    )
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query("DROP FUNCTION fail_count_operation_execution()")
        .execute(store.pool())
        .await
        .unwrap();
    let (status, result) = call(app, f.actor, "POST", &approve, command(&prepared)).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true);
    assert_eq!(result["updatedDocument"]["id"], id.to_string());
    let status: String =
        sqlx::query_scalar("SELECT status FROM business_document_approval_requests WHERE id=$1")
            .bind(
                result["requestId"]
                    .as_str()
                    .unwrap()
                    .parse::<Uuid>()
                    .unwrap(),
            )
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(status, "executed");
    let after = state(store, id).await;
    assert!(call(app, f.actor, "POST", &approve, command(&prepared))
        .await
        .0
        .is_client_error());
    assert_eq!(state(store, id).await, after);
    result
}
pub(super) async fn check(app: &Router, store: &PgStore, f: &Fixture) {
    for action in [
        "inventory_opening:create",
        "inventory_opening:post",
        "inventory_opening:reverse",
    ] {
        sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval,require_distinct_business_unit) VALUES($1,$1,ARRAY['b2_operator'],1,true,false) ON CONFLICT(action_code) DO NOTHING").bind(action).execute(store.pool()).await.unwrap();
    }
    let sku = Uuid::new_v4();
    sqlx::query("INSERT INTO business_skus(id,product_id,code,name) SELECT $1,product_id,'COUNT-APPROVALS','Count approval SKU' FROM business_skus WHERE id=$2").bind(sku).bind(f.sku).execute(store.pool()).await.unwrap();
    sqlx::query(
        "INSERT INTO inventory_balances(legal_entity_id,warehouse_id,sku_id) VALUES($1,$2,$3)",
    )
    .bind(f.legal_entity)
    .bind(f.warehouse)
    .bind(sku)
    .execute(store.pool())
    .await
    .unwrap();
    let service = InventoryCountService::new(store.clone(), "CNT".into());
    let input:CreateInventoryCount=serde_json::from_value(json!({"legalEntityId":f.legal_entity,"warehouseId":f.warehouse,"countDate":"2026-09-20","currency":"CNY","skuIds":[sku]})).unwrap();
    for actual in ["2", "0"] {
        let count = service
            .create(
                f.actor,
                Uuid::new_v4(),
                &format!("count-approvals-{actual}"),
                &input,
            )
            .await
            .unwrap();
        let detail = service.detail(f.actor, count.id).await.unwrap();
        exercise(app,store,f,count.id,"inventory_count_submission_intent",json!({"operation":"submit","command":{"expectedVersion":1,"lines":[{"countLineId":detail.lines[0].id,"actualOnHandQuantity":actual,"surplusUnitCost":"7"}]}})).await;
        exercise(
            app,
            store,
            f,
            count.id,
            "inventory_count_posting_intent",
            json!({"operation":"post","command":{"expectedVersion":2}}),
        )
        .await;
    }
    let count = service
        .create(f.actor, Uuid::new_v4(), "count-approvals-cancel", &input)
        .await
        .unwrap();
    exercise(app,store,f,count.id,"inventory_count_cancellation_intent",json!({"operation":"cancel","command":{"expectedVersion":1,"reasonCode":"cancel test count"}})).await;
    let balance: (Decimal, Decimal) = sqlx::query_as(
        "SELECT on_hand_quantity,inventory_value FROM inventory_balances WHERE sku_id=$1",
    )
    .bind(sku)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(balance, (Decimal::ZERO, Decimal::ZERO));
}
