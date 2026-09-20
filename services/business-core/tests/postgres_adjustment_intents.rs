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
    json!({"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve","sourceBuzzEventId":Uuid::new_v4().simple().to_string().repeat(2),"sourceChannelId":"isolated-adjustment-test"})
}
fn approval_path(kind: &str, prepared: &Value) -> String {
    format!(
        "/v1/agent-approvals/adjustments/{kind}/{}",
        prepared["item"]["id"].as_str().unwrap()
    )
}
async fn prepare(app: &Router, actor: Uuid, kind: &str, input: Value) -> Value {
    let key = Uuid::new_v4().to_string();
    let path = format!("/v1/agent-adjustment-intents/{kind}");
    let (status, result) = call(app, actor, "POST", &path, input.clone(), &key).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let (status, replay) = call(app, actor, "POST", &path, input, &key).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["item"]["id"], result["item"]["id"]);
    result
}

#[path = "support/adjustment_draft_intents.rs"]
mod drafts;
#[path = "support/adjustment_intent_expiry.rs"]
mod expiry;
#[path = "support/adjustment_intent_fixture.rs"]
mod fixture;
#[path = "support/adjustment_reversal_intents.rs"]
mod reversals;
const KIND: &str = "operational_adjustment_post_intent";
async fn counts(pool: &PgPool) -> (i64, i64, i64, i64, i64, i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM business_document_approval_requests),(SELECT count(*) FROM business_document_approval_votes),(SELECT count(*) FROM operational_adjustment_previews),(SELECT count(*) FROM operational_adjustment_allocations),(SELECT count(*) FROM profit_facts),(SELECT count(*) FROM business_core_audit_events),(SELECT count(*) FROM business_command_idempotency),(SELECT count(*) FROM business_core_outbox)").fetch_one(pool).await.unwrap()
}
#[tokio::test]
async fn adjustment_intents_bind_preview_votes_and_posting() {
    let Ok(url) = std::env::var("BUSINESS_CORE_ADJUSTMENT_INTENT_TEST_DATABASE_URL") else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(16)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let f = b2_seed::seed(&pool).await;
    let order = fixture::source(&pool, &f).await;
    let first = reviewer(&pool, &f).await;
    let second = reviewer(&pool, &f).await;
    let extra = Uuid::new_v4();
    sqlx::query("INSERT INTO business_brands(id,code,name) VALUES($1,'ADJ_REVIEWER_EXTRA','Reviewer extra scope')").bind(extra).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$3)").bind(first).bind(extra).bind(f.actor).execute(&pool).await.unwrap();
    let app = business_core::router(AppState::new(store.clone(), &Config::from_env().unwrap()));
    drafts::verify(&pool, &app, &f, order, first, second).await;
    let id = fixture::draft(&pool, &f, order, "adjustment-intent-draft").await;
    let input = json!({"batchId":id,"expectedVersion":1});
    let before = counts(&pool).await;
    let dry_path = format!("/v1/agent-adjustment-previews/{KIND}");
    let (code, dry) = call(&app, f.actor, "POST", &dry_path, input.clone(), "").await;
    assert_eq!(code, StatusCode::OK, "{dry}");
    assert_eq!(counts(&pool).await, before);
    let mut wrong = input.clone();
    wrong["amount"] = json!("999");
    assert_eq!(
        call(&app, f.actor, "POST", &dry_path, wrong, "").await.0,
        StatusCode::BAD_REQUEST
    );
    let prepared = prepare(&app, f.actor, KIND, input).await;
    assert_eq!(prepared["document"], dry["document"]);
    assert!(sqlx::query(
        "UPDATE business_agent_adjustment_intents SET snapshot=snapshot WHERE id=$1"
    )
    .bind(
        prepared["item"]["id"]
            .as_str()
            .unwrap()
            .parse::<Uuid>()
            .unwrap()
    )
    .execute(&pool)
    .await
    .is_err());
    let path = approval_path(KIND, &prepared);
    assert_eq!(
        call(&app, first, "POST", &path, vote(&prepared), "")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES('profit_adjustment:post','profit_adjustment:post',ARRAY['b2_operator'],2,false)").execute(&pool).await.unwrap();
    assert_eq!(
        call(&app, f.actor, "POST", &path, vote(&prepared), "")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let mut tampered = vote(&prepared);
    tampered["previewHash"] = json!("0".repeat(64));
    assert_eq!(
        call(&app, first, "POST", &path, tampered, "").await.0,
        StatusCode::CONFLICT
    );
    let (code, pending) = call(&app, first, "POST", &path, vote(&prepared), "").await;
    assert_eq!(code, StatusCode::OK, "{pending}");
    assert_eq!(pending["executed"], false);
    assert!(pending["postedDocument"].is_null());
    let waiting = counts(&pool).await;
    sqlx::query("UPDATE enterprise_users SET status='disabled' WHERE id=$1")
        .bind(first)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        call(&app, second, "POST", &path, vote(&prepared), "")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(counts(&pool).await, waiting);
    sqlx::query("UPDATE enterprise_users SET status='active' WHERE id=$1")
        .bind(first)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE FUNCTION fail_adjustment_vote() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.operation='chat_document_approval_vote' AND NEW.target_type='operational_adjustment_post_intent' AND (NEW.details->>'approvalCount')::int>=2 THEN RAISE EXCEPTION 'injected vote audit failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER fail_adjustment_vote BEFORE INSERT ON business_core_audit_events FOR EACH ROW EXECUTE FUNCTION fail_adjustment_vote();").execute(&pool).await.unwrap();
    assert_eq!(
        call(&app, second, "POST", &path, vote(&prepared), "")
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(counts(&pool).await, waiting);
    sqlx::raw_sql("DROP TRIGGER fail_adjustment_vote ON business_core_audit_events; DROP FUNCTION fail_adjustment_vote();").execute(&pool).await.unwrap();
    sqlx::query("UPDATE business_approval_policies SET min_approvers=1 WHERE action_code='profit_adjustment:post'").execute(&pool).await.unwrap();
    let (code, done) = call(&app, second, "POST", &path, vote(&prepared), "").await;
    assert_eq!(code, StatusCode::OK, "{done}");
    assert_eq!(done["executed"], true);
    assert_eq!(done["minimumApprovers"], 2);
    assert_eq!(done["postedDocument"]["id"], json!(id));
    assert_eq!(done["postedDocument"]["status"], "posted");
    let facts:i64=sqlx::query_scalar("SELECT count(*) FROM profit_facts WHERE source_type='operational_adjustment' AND source_id=$1").bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(facts, 1);
    assert_eq!(
        call(&app, second, "POST", &path, vote(&prepared), "")
            .await
            .0,
        StatusCode::CONFLICT
    );
    let id = fixture::draft(&pool, &f, order, "adjustment-reject-draft").await;
    let prepared = prepare(
        &app,
        f.actor,
        KIND,
        json!({"batchId":id,"expectedVersion":1}),
    )
    .await;
    let mut rejection = vote(&prepared);
    rejection["decision"] = json!("reject");
    let (code, rejected) = call(
        &app,
        first,
        "POST",
        &approval_path(KIND, &prepared),
        rejection,
        "",
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{rejected}");
    assert_eq!(rejected["status"], "rejected");
    assert!(rejected["postedDocument"].is_null());
    let status: String =
        sqlx::query_scalar("SELECT status FROM operational_adjustment_batches WHERE id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "draft");
    let mut disabled = Config::from_env().unwrap();
    disabled.operational_adjustments_enabled = false;
    let disabled = business_core::router(AppState::new(store, &disabled));
    assert_eq!(
        call(
            &disabled,
            f.actor,
            "POST",
            &dry_path,
            json!({"batchId":id,"expectedVersion":1}),
            ""
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let id = fixture::draft(&pool, &f, order, "adjustment-concurrent-voters-draft").await;
    sqlx::query("UPDATE business_approval_policies SET min_approvers=2 WHERE action_code='profit_adjustment:post'").execute(&pool).await.unwrap();
    let prepared = prepare(
        &app,
        f.actor,
        KIND,
        json!({"batchId":id,"expectedVersion":1}),
    )
    .await;
    let path = approval_path(KIND, &prepared);
    let (a, b) = tokio::join!(
        call(&app, first, "POST", &path, vote(&prepared), ""),
        call(&app, second, "POST", &path, vote(&prepared), "")
    );
    assert_eq!(a.0, StatusCode::OK, "{}", a.1);
    assert_eq!(b.0, StatusCode::OK, "{}", b.1);
    assert_ne!(a.1["executed"], b.1["executed"]);
    let facts:i64=sqlx::query_scalar("SELECT count(*) FROM profit_facts WHERE source_type='operational_adjustment' AND source_id=$1").bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(facts, 1);
    sqlx::query("UPDATE business_approval_policies SET min_approvers=1 WHERE action_code='profit_adjustment:post'").execute(&pool).await.unwrap();
    expiry::verify(&pool, &app, &f, order, first).await;
    reversals::verify(&pool, &app, &f, order, first, second).await;
}
async fn reviewer(pool: &PgPool, f: &Fixture) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'adjustment-reviewer',$1::text,'Reviewer')").bind(id).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) SELECT $1,role_id,$2 FROM business_user_roles WHERE enterprise_user_id=$2").bind(id).bind(f.actor).execute(pool).await.unwrap();
    for (table, column) in [
        ("business_legal_entity_scopes", "legal_entity_id"),
        ("business_customer_scopes", "customer_id"),
        ("business_brand_scopes", "brand_id"),
        ("business_unit_scopes", "business_unit_id"),
        ("business_warehouse_scopes", "warehouse_id"),
        ("business_supplier_scopes", "supplier_id"),
    ] {
        let sql=format!("INSERT INTO {table}(enterprise_user_id,{column},granted_by) SELECT $1,{column},$2 FROM {table} WHERE enterprise_user_id=$2");
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id)
            .bind(f.actor)
            .execute(pool)
            .await
            .unwrap();
    }
    id
}
