use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use business_core::{AppState, Config, PgStore};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;
#[path = "crm_intent_failures.rs"]
mod failures;

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
    let body = to_bytes(response.into_body(), 1000000).await.unwrap();
    (
        status,
        serde_json::from_slice(&body)
            .unwrap_or_else(|_| json!({"raw":String::from_utf8_lossy(&body)})),
    )
}
fn vote(prepared: &Value) -> Value {
    json!({"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve",
        "sourceBuzzEventId":Uuid::new_v4().simple().to_string().repeat(2),"sourceChannelId":"isolated-crm-test"})
}
async fn counts(pool: &PgPool) -> (i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM crm_opportunities),(SELECT count(*) FROM crm_followups),(SELECT count(*) FROM sales_orders)")
        .fetch_one(pool).await.unwrap()
}
pub async fn check(pool: &PgPool, actor: Uuid, customer: Uuid, legal: Uuid, unit: Uuid) {
    let config =
        Config::from_env().expect("CRM approval tests require explicit isolated service config");
    let app = business_core::router(AppState::new(PgStore::new(pool.clone()), &config));
    sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)")
        .bind(actor).bind(customer).execute(pool).await.unwrap();
    let fields = json!({"legalEntityId":legal,"businessUnitId":unit,"customerId":customer,
        "title":"Agent CRM","companyName":"CRM acceptance","stage":"new","currency":"CNY","expectedAmountMinor":20000});
    let create = json!({"operation":"create","command":fields});
    let before = counts(pool).await;
    let (status, dry) = call(
        &app,
        actor,
        "POST",
        "/v1/agent-crm-previews/crm_creation_intent",
        create.clone(),
        "crm-dry-preview-0001",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dry}");
    assert_eq!(counts(pool).await, before);
    let mut id = Uuid::nil();
    for (n, kind) in [
        "crm_creation_intent",
        "crm_update_intent",
        "crm_followup_intent",
    ]
    .into_iter()
    .enumerate()
    {
        let input = match n {
            0 => create.clone(),
            1 => {
                let mut fields = fields.clone();
                fields["title"] = "Updated by approved CRM intent".into();
                fields["stage"] = "quoting".into();
                fields["expectedVersion"] = 1.into();
                json!({"operation":"update","opportunityId":id,"command":fields})
            }
            _ => json!({"operation":"followup","opportunityId":id,"command":{
                "note":"Confirmed requirements","stage":"won","nextAction":"Prepare sales order",
                "expectedVersion":2
            }}),
        };
        let path = format!("/v1/agent-crm-intents/{kind}");
        let key = format!("crm-intent-test-{n:04}");
        let mut unknown = input.clone();
        unknown["command"]["execute"] = true.into();
        assert!(call(&app, actor, "POST", &path, unknown, &key)
            .await
            .0
            .is_client_error());
        let (status, prepared) = call(&app, actor, "POST", &path, input.clone(), &key).await;
        assert_eq!(status, StatusCode::OK, "{prepared}");
        let (_, again) = call(&app, actor, "POST", &path, input.clone(), &key).await;
        assert_eq!(again["item"]["id"], prepared["item"]["id"]);
        let mut changed = input.clone();
        if n == 2 {
            changed["command"]["note"] = "different".into();
        } else {
            changed["command"]["title"] = "different".into();
        }
        assert_eq!(
            call(&app, actor, "POST", &path, changed, &key).await.0,
            StatusCode::CONFLICT
        );
        let intent: Uuid = serde_json::from_value(prepared["item"]["id"].clone()).unwrap();
        assert!(
            sqlx::query("UPDATE business_agent_crm_intents SET snapshot='{}' WHERE id=$1")
                .bind(intent)
                .execute(pool)
                .await
                .is_err()
        );
        let approve = format!("/v1/agent-approvals/crm/{kind}/{intent}");
        let preview = format!("/v1/agent-approval-previews/crm/{kind}/{intent}");
        if n == 0 {
            // A missing policy must not silently become permission to execute.
            assert!(call(&app, actor, "POST", &approve, vote(&prepared), &key)
                .await
                .0
                .is_client_error());
            sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES('crm:manage','crm:manage',ARRAY['crm_test'],1,true)")
                .execute(pool).await.unwrap();
        }
        let mut bad = vote(&prepared);
        bad["previewHash"] = "0".repeat(64).into();
        assert_eq!(
            call(&app, actor, "POST", &approve, bad, &key).await.0,
            StatusCode::CONFLICT
        );
        let mut extra = vote(&prepared);
        extra["command"] = input.clone();
        assert!(call(&app, actor, "POST", &approve, extra, &key)
            .await
            .0
            .is_client_error());
        sqlx::query(
            "DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2",
        )
        .bind(actor)
        .bind(customer)
        .execute(pool)
        .await
        .unwrap();
        assert_eq!(
            call(&app, actor, "GET", &preview, Value::Null, &key)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(&app, actor, "POST", &approve, vote(&prepared), &key)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)")
            .bind(actor).bind(customer).execute(pool).await.unwrap();
        let (status, result) = call(&app, actor, "POST", &approve, vote(&prepared), &key).await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(result["status"], "executed");
        assert_eq!(result["createdDocument"]["version"], (n + 1) as i64);
        id = serde_json::from_value(result["createdDocument"]["id"].clone()).unwrap();
        assert!(call(&app, actor, "POST", &approve, vote(&prepared), &key)
            .await
            .0
            .is_client_error());
        let status: String = sqlx::query_scalar(
            "SELECT status FROM business_document_approval_requests WHERE document_id=$1",
        )
        .bind(intent)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(status, "executed");
    }
    assert_eq!(counts(pool).await, (before.0 + 1, before.1 + 1, before.2));
    let stage: String = sqlx::query_scalar("SELECT stage FROM crm_opportunities WHERE id=$1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(stage, "won");
    failures::check(&app, pool, actor, customer, id).await;
    // Rejection saves a vote, but creates no opportunity.
    let (_, prepared) = call(
        &app,
        actor,
        "POST",
        "/v1/agent-crm-intents/crm_creation_intent",
        create.clone(),
        "crm-rejection-0001",
    )
    .await;
    let mut rejection = vote(&prepared);
    rejection["decision"] = "reject".into();
    let path = format!(
        "/v1/agent-approvals/crm/crm_creation_intent/{}",
        prepared["item"]["id"].as_str().unwrap()
    );
    let current = counts(pool).await;
    let (status, result) = call(&app, actor, "POST", &path, rejection, "crm-rejection-0001").await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], false);
    assert_eq!(counts(pool).await, current);
    // Expired intents cannot be read or approved.
    let expired = Uuid::new_v4();
    sqlx::query("INSERT INTO business_agent_crm_intents(id,kind,input,snapshot,created_by_user_id,idempotency_key,trace_id,expires_at) SELECT $1,kind,input,snapshot,created_by_user_id,'crm-expired-intent',trace_id,now()-interval '1 second' FROM business_agent_crm_intents WHERE id=$2")
        .bind(expired).bind(Uuid::parse_str(prepared["item"]["id"].as_str().unwrap()).unwrap()).execute(pool).await.unwrap();
    let path = format!("/v1/agent-approvals/crm/crm_creation_intent/{expired}");
    assert_eq!(
        call(
            &app,
            actor,
            "POST",
            &path,
            vote(&prepared),
            "crm-expired-0001"
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}
