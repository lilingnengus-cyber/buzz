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
    json!({"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve","sourceBuzzEventId":Uuid::new_v4().simple().to_string().repeat(2),"sourceChannelId":"isolated-operating-snapshot-test"})
}
fn approval_path(kind: &str, prepared: &Value) -> String {
    format!(
        "/v1/agent-approvals/operating-snapshots/{kind}/{}",
        prepared["item"]["id"].as_str().unwrap()
    )
}
async fn prepare(app: &Router, actor: Uuid, kind: &str, input: Value) -> Value {
    let key = Uuid::new_v4().to_string();
    let path = format!("/v1/agent-operating-snapshot-intents/{kind}");
    let (status, result) = call(app, actor, "POST", &path, input.clone(), &key).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let (status, replay) = call(app, actor, "POST", &path, input, &key).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["item"]["id"], result["item"]["id"]);
    result
}

#[tokio::test]
async fn operating_reports_bind_requester_scope_and_independent_reviewers() {
    let Ok(url) = std::env::var("BUSINESS_CORE_OPERATING_INTENT_TEST_DATABASE_URL") else {
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
    for permission in [
        "management_report:generate_snapshot",
        "management_report:read",
    ] {
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,$2 FROM business_user_roles WHERE enterprise_user_id=$1").bind(f.actor).bind(permission).execute(&pool).await.unwrap();
    }
    let first = reviewer(&pool, &f).await;
    let second = reviewer(&pool, &f).await;
    // Reviewer can see additional data, but must approve the requester's report.
    let extra = Uuid::new_v4();
    sqlx::query("INSERT INTO business_brands(id,code,name) VALUES($1,'OP_REVIEWER_EXTRA','Additional reviewer brand')").bind(extra).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$3)").bind(first).bind(extra).bind(f.actor).execute(&pool).await.unwrap();
    let app = business_core::router(AppState::new(store, &Config::from_env().unwrap()));
    let kind = "operating_report_snapshot_intent";
    for cadence in ["daily", "weekly"] {
        let input = json!({"cadence":cadence,"periodStart":"2026-01-19","currency":"CNY","utcOffsetMinutes":480});
        let before = counts(&pool).await;
        let (code, dry) = call(
            &app,
            f.actor,
            "POST",
            &format!("/v1/agent-operating-snapshot-previews/{kind}"),
            input.clone(),
            "",
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{dry}");
        assert_eq!(before, counts(&pool).await);
        let prepared = prepare(&app, f.actor, kind, input.clone()).await;
        assert_eq!(prepared["document"]["ownerUserId"], json!(f.actor));
        let preview_path = format!(
            "/v1/agent-approval-previews/operating-snapshots/{kind}/{}",
            prepared["item"]["id"].as_str().unwrap()
        );
        let (code, view) = call(&app, first, "GET", &preview_path, Value::Null, "").await;
        assert_eq!(code, StatusCode::OK, "{view}");
        assert_eq!(view["document"], prepared["document"]);
        let path = approval_path(kind, &prepared);
        if cadence == "daily" {
            assert_eq!(
                call(&app, first, "POST", &path, vote(&prepared), "")
                    .await
                    .0,
                StatusCode::NOT_FOUND
            );
            assert_eq!(before, counts(&pool).await);
        }
        sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES('management_report:generate_snapshot','management_report:generate_snapshot',ARRAY['b2_operator'],2,false) ON CONFLICT(action_code) DO UPDATE SET min_approvers=2,allow_self_approval=false").execute(&pool).await.unwrap();
        // Policy changes revise the owner scope identity; require a fresh intent.
        assert_eq!(
            call(&app, first, "POST", &path, vote(&prepared), "")
                .await
                .0,
            StatusCode::CONFLICT
        );
        let prepared = prepare(&app, f.actor, kind, input).await;
        let path = approval_path(kind, &prepared);
        assert_eq!(
            call(&app, f.actor, "POST", &path, vote(&prepared), "")
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        let (code, pending) = call(&app, first, "POST", &path, vote(&prepared), "").await;
        assert_eq!(code, StatusCode::OK, "{pending}");
        assert_eq!(pending["executed"], false);
        assert!(pending["createdDocument"].is_null());
        let voted = counts(&pool).await;
        assert_eq!(voted, (before.0, before.1 + 1, before.2 + 1));
        // Inactive prior reviewers cannot provide a retained approval witness.
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
        assert_eq!(voted, counts(&pool).await);
        sqlx::query("UPDATE enterprise_users SET status='active' WHERE id=$1")
            .bind(first)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql("CREATE FUNCTION fail_operating_approval() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.operation='chat_document_approval_vote' AND NEW.target_type='operating_report_snapshot_intent' AND (NEW.details->>'approvalCount')::int >= 2 THEN RAISE EXCEPTION 'injected operating approval failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER fail_operating_approval BEFORE INSERT ON business_core_audit_events FOR EACH ROW EXECUTE FUNCTION fail_operating_approval();").execute(&pool).await.unwrap();
        assert_eq!(
            call(&app, second, "POST", &path, vote(&prepared), "")
                .await
                .0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(voted, counts(&pool).await);
        sqlx::raw_sql("DROP TRIGGER fail_operating_approval ON business_core_audit_events; DROP FUNCTION fail_operating_approval();").execute(&pool).await.unwrap();
        let (code, done) = call(&app, second, "POST", &path, vote(&prepared), "").await;
        assert_eq!(code, StatusCode::OK, "{done}");
        assert_eq!(done["executed"], true);
        assert_eq!(done["minimumApprovers"], 2);
        assert_eq!(done["approvalCount"], 2);
        assert_eq!(done["createdDocument"]["ownerUserId"], json!(f.actor));
        assert_eq!(
            done["createdDocument"]["sourceHash"],
            prepared["document"]["sourceHash"]
        );
        let id: Uuid = done["createdDocument"]["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let (owner, payload): (Uuid, Value) = sqlx::query_as(
            "SELECT generated_by_user_id,payload FROM operating_report_snapshots WHERE id=$1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(owner, f.actor);
        assert_eq!(payload, prepared["document"]["metrics"]);
        assert_eq!(
            counts(&pool).await,
            (before.0 + 1, before.1 + 1, before.2 + 2)
        );
        assert_eq!(
            call(&app, second, "POST", &path, vote(&prepared), "")
                .await
                .0,
            StatusCode::CONFLICT
        );
    }
}
async fn counts(pool: &PgPool) -> (i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM operating_report_snapshots),(SELECT count(*) FROM business_document_approval_requests),(SELECT count(*) FROM business_document_approval_votes)").fetch_one(pool).await.unwrap()
}
async fn reviewer(pool: &PgPool, f: &Fixture) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'operating-reviewer',$1::text,'Reviewer')").bind(id).execute(pool).await.unwrap();
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
