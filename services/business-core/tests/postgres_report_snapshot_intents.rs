use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use business_core::{AppState, Config, PgStore};
use serde_json::{json, Value};
use sqlx::{postgres::PgPoolOptions, PgPool};
use tower::ServiceExt;
use uuid::Uuid;
#[path = "support/b2_seed.rs"]
mod b2_seed;
struct Fixture {
    actor: Uuid,
    legal_entity: Uuid,
    business_unit: Uuid,
    warehouse: Uuid,
    customer: Uuid,
    brand: Uuid,
    uom: Uuid,
    sku: Uuid,
}
async fn call(
    app: &Router,
    actor: Uuid,
    method: &str,
    path: &str,
    input: Value,
    key: &str,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(
                    "x-business-service-credential",
                    std::env::var("BUSINESS_CORE_SERVICE_CREDENTIAL").unwrap(),
                )
                .header("x-service-audience", "business-core")
                .header("x-enterprise-user-id", actor.to_string())
                .header("x-trace-id", Uuid::new_v4().to_string())
                .header("content-type", "application/json")
                .header("idempotency-key", key)
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1000000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"body":String::from_utf8_lossy(&bytes)})),
    )
}
fn vote(prepared: &Value) -> Value {
    json!({"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve","sourceBuzzEventId":Uuid::new_v4().simple().to_string().repeat(2),"sourceChannelId":"isolated-report-snapshot-test"})
}
fn approval_path(kind: &str, prepared: &Value) -> String {
    format!(
        "/v1/agent-approvals/report-snapshots/{kind}/{}",
        prepared["item"]["id"].as_str().unwrap()
    )
}
async fn prepare(app: &Router, actor: Uuid, kind: &str, input: Value) -> Value {
    let key = Uuid::new_v4().to_string();
    let path = format!("/v1/agent-report-snapshot-intents/{kind}");
    let (status, result) = call(app, actor, "POST", &path, input.clone(), &key).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let (status, replay) = call(app, actor, "POST", &path, input, &key).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["item"]["id"], result["item"]["id"]);
    result
}

#[tokio::test]
async fn monthly_report_intent_approval_is_atomic() {
    let Ok(url) = std::env::var("BUSINESS_CORE_REPORT_INTENT_TEST_DATABASE_URL") else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let f = b2_seed::seed(&pool).await;
    let actor = f.actor;
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,'management_report:generate_snapshot' FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(actor).execute(&pool).await.unwrap();
    let app = business_core::router(AppState::new(store, &Config::from_env().unwrap()));
    let kind = "management_report_snapshot_intent";
    let input = json!({"reportType":"management_profit_statement","managementPeriod":"2026-08","currency":"CNY","legalEntityIds":[f.legal_entity]});
    let (code, dry) = call(
        &app,
        actor,
        "POST",
        &format!("/v1/agent-report-snapshot-previews/{kind}"),
        input.clone(),
        "",
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{dry}");
    assert_eq!(counts(&pool).await, (0, 0, 0));
    let prepared = prepare(&app, actor, kind, input.clone()).await;
    assert!(
        sqlx::query("UPDATE business_agent_report_snapshot_intents SET input='{}'")
            .execute(&pool)
            .await
            .is_err()
    );
    let stale = prepare(&app, actor, kind, input.clone()).await;
    let path = approval_path(kind, &prepared);
    assert_eq!(
        call(&app, actor, "POST", &path, vote(&prepared), "")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(counts(&pool).await, (0, 0, 0));
    sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES('management_report:generate_snapshot','management_report:generate_snapshot',ARRAY['b2_operator'],1,true)").execute(&pool).await.unwrap();
    let mut bad = vote(&prepared);
    bad["previewHash"] = json!("0".repeat(64));
    assert_eq!(
        call(&app, actor, "POST", &path, bad, "").await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(counts(&pool).await, (0, 0, 0));
    // Force failure after report creation to verify outer approval atomicity.
    sqlx::query("CREATE FUNCTION fail_report_vote() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.operation='chat_document_approval_vote' THEN RAISE EXCEPTION 'injected report approval failure'; END IF; RETURN NEW; END $$").execute(&pool).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_report_vote BEFORE INSERT ON business_core_audit_events FOR EACH ROW EXECUTE FUNCTION fail_report_vote()").execute(&pool).await.unwrap();
    assert_eq!(
        call(&app, actor, "POST", &path, vote(&prepared), "")
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(counts(&pool).await, (0, 0, 0));
    sqlx::query("DROP TRIGGER fail_report_vote ON business_core_audit_events")
        .execute(&pool)
        .await
        .unwrap();
    let (code, result) = call(&app, actor, "POST", &path, vote(&prepared), "").await;
    assert_eq!(code, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true);
    assert_eq!(result["createdDocument"]["status"], "generated");
    assert_eq!(counts(&pool).await, (1, 1, 1));
    assert_eq!(
        call(
            &app,
            actor,
            "POST",
            &approval_path(kind, &stale),
            vote(&stale),
            ""
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(counts(&pool).await, (1, 1, 1));

    assert_eq!(
        call(&app, actor, "POST", &path, vote(&prepared), "")
            .await
            .0,
        StatusCode::CONFLICT
    );
    let reused = prepare(&app, actor, kind, input).await;
    let mut rejection = vote(&reused);
    rejection["decision"] = json!("reject");
    let (code, result) = call(
        &app,
        actor,
        "POST",
        &approval_path(kind, &reused),
        rejection,
        "",
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], false);
    assert!(result["createdDocument"].is_null());
    assert_eq!(counts(&pool).await, (1, 2, 2));
}
async fn counts(pool: &PgPool) -> (i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM management_report_snapshots),(SELECT count(*) FROM business_document_approval_requests),(SELECT count(*) FROM business_document_approval_votes)").fetch_one(pool).await.unwrap()
}
