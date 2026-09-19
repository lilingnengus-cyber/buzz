use super::*;
const KIND: &str = "inventory_count_creation_intent";
fn command(preview: &Value) -> Value {
    json!({"expectedVersion":1,"previewHash":preview["previewHash"],"decision":"approve","sourceBuzzEventId":Uuid::new_v4().simple().to_string().repeat(2),"sourceChannelId":"count-creation-test"})
}
async fn count(store: &PgStore) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM inventory_count_tasks")
        .fetch_one(store.pool())
        .await
        .unwrap()
}
pub(super) async fn check(app: &Router, store: &PgStore, f: &Fixture) {
    let before = count(store).await;
    let input = json!({"legalEntityId":f.legal_entity,"warehouseId":f.warehouse,"countDate":"2026-09-19","currency":"CNY","skuIds":[f.sku]});
    let prepare = format!("/v1/agent-inventory-count-creation-intents/{KIND}");
    let dry = format!("/v1/agent-inventory-count-creation-previews/{KIND}");
    assert_eq!(
        call(app, f.actor, "POST", &dry, input.clone()).await.0,
        StatusCode::OK
    );
    let mut invalid = input.clone();
    invalid["execute"] = true.into();
    assert!(call(app, f.actor, "POST", &prepare, invalid)
        .await
        .0
        .is_client_error());
    let (status, prepared) = call_key(
        app,
        f.actor,
        "POST",
        &prepare,
        input.clone(),
        "count-intent-first",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{prepared}");
    let id = prepared["item"]["id"].as_str().unwrap();
    let preview = format!("/v1/agent-approval-previews/inventory-count-creations/{KIND}/{id}");
    let approve = format!("/v1/agent-approvals/inventory-count-creations/{KIND}/{id}");
    let (_, replay) = call_key(
        app,
        f.actor,
        "POST",
        &prepare,
        input.clone(),
        "count-intent-first",
    )
    .await;
    assert_eq!(replay["item"]["id"], prepared["item"]["id"]);
    let mut changed = input.clone();
    changed["businessNote"] = "changed".into();
    assert_eq!(
        call_key(
            app,
            f.actor,
            "POST",
            &prepare,
            changed,
            "count-intent-first"
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(count(store).await, before);
    assert!(sqlx::query(
        "UPDATE business_agent_inventory_count_creation_intents SET snapshot='{}' WHERE id=$1"
    )
    .bind(id.parse::<Uuid>().unwrap())
    .execute(store.pool())
    .await
    .is_err());
    let expired = Uuid::new_v4();
    sqlx::query("INSERT INTO business_agent_inventory_count_creation_intents(id,kind,input,snapshot,created_by_user_id,idempotency_key,expires_at,trace_id) SELECT $2,kind,input,snapshot,created_by_user_id,'expired-count-intent',now()-interval '1 minute',trace_id FROM business_agent_inventory_count_creation_intents WHERE id=$1").bind(id.parse::<Uuid>().unwrap()).bind(expired).execute(store.pool()).await.unwrap();
    assert_eq!(
        call(
            app,
            f.actor,
            "GET",
            &format!("/v1/agent-approval-previews/inventory-count-creations/{KIND}/{expired}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(app, f.actor, "POST", &approve, command(&prepared))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval,require_distinct_business_unit) VALUES('inventory_opening:create','inventory_opening:create',ARRAY['b2_operator'],1,true,false)").execute(store.pool()).await.unwrap();
    let mut stale = command(&prepared);
    stale["previewHash"] = "0".repeat(64).into();
    assert_eq!(
        call(app, f.actor, "POST", &approve, stale).await.0,
        StatusCode::CONFLICT
    );
    let mut extra = command(&prepared);
    extra["skuIds"] = json!([]);
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
    let mut reject = command(&rejected);
    reject["decision"] = "reject".into();
    let (_, result) = call(
        app,
        f.actor,
        "POST",
        &format!(
            "/v1/agent-approvals/inventory-count-creations/{KIND}/{}",
            rejected["item"]["id"].as_str().unwrap()
        ),
        reject,
    )
    .await;
    assert_eq!(result["executed"], false, "{result}");
    assert_eq!(count(store).await, before);
    let (_, failed) = call(app, f.actor, "POST", &prepare, input.clone()).await;
    sqlx::query("CREATE FUNCTION fail_count_execution_result() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.document_type='inventory_count_creation_intent' AND NEW.status='executed' THEN RAISE EXCEPTION 'injected final approval failure'; END IF; RETURN NEW; END $$").execute(store.pool()).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_count_execution_result AFTER UPDATE ON business_document_approval_requests FOR EACH ROW EXECUTE FUNCTION fail_count_execution_result()").execute(store.pool()).await.unwrap();
    let failed_path = format!(
        "/v1/agent-approvals/inventory-count-creations/{KIND}/{}",
        failed["item"]["id"].as_str().unwrap()
    );
    let (status, result) = call(app, f.actor, "POST", &failed_path, command(&failed)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{result}");
    assert_eq!(
        count(store).await,
        before,
        "approval failure must roll back the count freeze"
    );
    sqlx::query("DROP TRIGGER fail_count_execution_result ON business_document_approval_requests")
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION fail_count_execution_result()")
        .execute(store.pool())
        .await
        .unwrap();
    let (status, result) = call(app, f.actor, "POST", &approve, command(&prepared)).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true, "{result}");
    assert_eq!(count(store).await, before + 1);
    let state: String =
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
    assert_eq!(state, "executed");
    assert!(call(app, f.actor, "POST", &approve, command(&prepared))
        .await
        .0
        .is_client_error());
    assert_eq!(count(store).await, before + 1);
    let count_id = result["createdDocument"]["id"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    business_core::b2::InventoryCountService::new(store.clone(), "CNT".into())
        .cancel(
            f.actor,
            Uuid::new_v4(),
            count_id,
            "count-intent-cleanup",
            &business_core::b2::model::VersionCommand {
                expected_version: 1,
                reason_code: Some("test cleanup".into()),
            },
        )
        .await
        .unwrap();
}
