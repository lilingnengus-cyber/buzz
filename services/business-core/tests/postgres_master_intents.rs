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
#[path = "support/master_intent_iam.rs"]
mod iam;
#[path = "support/master_intent_policies.rs"]
mod policies;
#[path = "support/master_intent_waits.rs"]
mod waits;

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
    (status, serde_json::from_slice(&bytes).unwrap())
}
fn vote(prepared: &Value) -> Value {
    json!({"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve","sourceBuzzEventId":Uuid::new_v4().simple().to_string().repeat(2),"sourceChannelId":"isolated-master-test"})
}
fn approval_path(kind: &str, prepared: &Value) -> String {
    format!(
        "/v1/agent-approvals/master/{kind}/{}",
        prepared["item"]["id"].as_str().unwrap()
    )
}
async fn prepare(app: &Router, actor: Uuid, kind: &str, input: Value) -> Value {
    let key = Uuid::new_v4().to_string();
    let path = format!("/v1/agent-master-intents/{kind}");
    let (status, result) = call(app, actor, "POST", &path, input.clone(), &key).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let (status, replay) = call(app, actor, "POST", &path, input, &key).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["item"]["id"], result["item"]["id"]);
    result
}
async fn approved(app: &Router, actor: Uuid, kind: &str, input: Value) -> Uuid {
    let prepared = prepare(app, actor, kind, input).await;
    let path = approval_path(kind, &prepared);
    let command = vote(&prepared);
    let (status, result) = call(app, actor, "POST", &path, command.clone(), "").await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["status"], "executed");
    assert_eq!(result["executed"], true);
    assert_ne!(
        call(app, actor, "POST", &path, command, "").await.0,
        StatusCode::OK
    );
    Uuid::parse_str(result["createdDocument"]["id"].as_str().unwrap()).unwrap()
}
#[tokio::test]
async fn master_intents_execute_atomically_under_current_policy() {
    let Ok(url) = std::env::var("BUSINESS_CORE_MASTER_INTENT_TEST_DATABASE_URL") else {
        eprintln!("skipping: BUSINESS_CORE_MASTER_INTENT_TEST_DATABASE_URL unset");
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect(&url)
        .await
        .unwrap();
    PgStore::new(pool.clone()).migrate().await.unwrap();
    let actor = Uuid::new_v4();
    let role = Uuid::new_v4();
    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'master-intents',$1::text,'Master requester')").bind(actor).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_roles(id,role_key,name) VALUES($1,'master_intent_approver','Master approver')").bind(role).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,'business_master_data:manage'),($1,'business_product_master:manage')").bind(role).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) VALUES($1,$2,$1)",
    )
    .bind(actor)
    .bind(role)
    .execute(&pool)
    .await
    .unwrap();
    let config = Config::from_env().expect("explicit isolated service configuration required");
    let app = business_core::router(AppState::new(PgStore::new(pool.clone()), &config));
    // Global creation has no made-up legal entity and no policy is auto-granted.
    let fields = json!({"resourceType":"legal_entity","code":"MI_LE","name":"Intent legal","countryCode":"CN","functionalCurrency":"CNY"});
    let command = json!({"operation":"create","command":fields});
    let prepared = prepare(&app, actor, "core_master_creation_intent", command.clone()).await;
    assert!(prepared["document"]["legalEntityId"].is_null());
    assert_eq!(
        call(
            &app,
            actor,
            "POST",
            &approval_path("core_master_creation_intent", &prepared),
            vote(&prepared),
            ""
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let requests: i64 =
        sqlx::query_scalar("SELECT count(*) FROM business_document_approval_requests")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(requests, 0);
    sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES('business_master_data:manage','business_master_data:manage',ARRAY['master_intent_approver'],1,true),('business_product_master:manage','business_product_master:manage',ARRAY['master_intent_approver'],1,true)").execute(&pool).await.unwrap();
    let legal = approved(&app, actor, "core_master_creation_intent", command).await;
    let mut entries = vec![(false, legal, fields)];
    let fields =
        json!({"resourceType":"business_unit","code":"MI_BU","name":"Unit","legalEntityId":legal});
    let unit = approved(
        &app,
        actor,
        "core_master_creation_intent",
        json!({"operation":"create","command":fields}),
    )
    .await;
    entries.push((false, unit, fields));
    for kind in ["customer", "supplier", "warehouse"] {
        let mut fields = json!({"resourceType":kind,"code":format!("MI_{kind}").to_uppercase(),"name":kind,"legalEntityId":legal,"businessUnitId":unit});
        if kind == "customer" {
            fields["creditCurrency"] = json!("CNY");
        }
        let id = approved(
            &app,
            actor,
            "core_master_creation_intent",
            json!({"operation":"create","command":fields}),
        )
        .await;
        entries.push((false, id, fields));
    }
    let mut product_ids = Vec::new();
    for (kind, code) in [
        ("unit_of_measure", "MI_EA"),
        ("unit_of_measure", "MI_BOX"),
        ("product_category", "MI_CATEGORY"),
        ("brand", "MI_BRAND"),
    ] {
        let mut fields = json!({"resourceType":kind,"code":code,"name":kind});
        if kind == "unit_of_measure" {
            fields["precisionScale"] = json!(0);
        }
        let id = approved(
            &app,
            actor,
            "product_master_creation_intent",
            json!({"operation":"create","command":fields}),
        )
        .await;
        product_ids.push(id);
        if code != "MI_BOX" {
            entries.push((true, id, fields));
        }
    }
    let fields = json!({"resourceType":"product","code":"MI_PRODUCT","name":"Product","categoryId":product_ids[2],"brandId":product_ids[3],"baseUomId":product_ids[0]});
    let product = approved(
        &app,
        actor,
        "product_master_creation_intent",
        json!({"operation":"create","command":fields}),
    )
    .await;
    entries.push((true, product, fields));
    let fields = json!({"resourceType":"sku","code":"MI_SKU","name":"SKU","productId":product});
    let sku = approved(
        &app,
        actor,
        "product_master_creation_intent",
        json!({"operation":"create","command":fields}),
    )
    .await;
    entries.push((true, sku, fields));
    let fields = json!({"resourceType":"uom_conversion","code":"","name":"","productId":product,"unitOfMeasureId":product_ids[1],"factorToBase":"0.33333333","usageScope":"both"});
    let conversion = approved(
        &app,
        actor,
        "product_master_creation_intent",
        json!({"operation":"create","command":fields}),
    )
    .await;
    entries.push((true, conversion, fields));
    assert_eq!(entries.len(), 11);
    for (is_product, id, fields) in &entries {
        let kind = if *is_product {
            "product_master_update_intent"
        } else {
            "core_master_update_intent"
        };
        let mut fields = fields.clone();
        fields["expectedVersion"] = json!(1);
        if fields["resourceType"] != "uom_conversion" {
            fields["name"] = json!("Approved update");
        }
        let updated = approved(
            &app,
            actor,
            kind,
            json!({"operation":"update","documentId":id,"command":fields}),
        )
        .await;
        assert_eq!(updated, *id);
    }
    let executed:i64=sqlx::query_scalar("SELECT count(*) FROM business_document_approval_requests WHERE status='executed' AND executed_at IS NOT NULL").fetch_one(&pool).await.unwrap();
    assert_eq!(executed, 23);
    let writes:i64=sqlx::query_scalar("SELECT count(*) FROM business_core_audit_events WHERE operation IN ('CORE_MASTER_DATA_SAVED','PRODUCT_MASTER_DATA_SAVED')").fetch_one(&pool).await.unwrap();
    assert_eq!(writes, 23);
    let update = sqlx::query("UPDATE business_agent_master_intents SET input='{}' WHERE id=$1")
        .bind(Uuid::parse_str(prepared["item"]["id"].as_str().unwrap()).unwrap())
        .execute(&pool)
        .await;
    assert!(update.is_err());
    let delete = sqlx::query("DELETE FROM business_agent_master_intents WHERE id=$1")
        .bind(Uuid::parse_str(prepared["item"]["id"].as_str().unwrap()).unwrap())
        .execute(&pool)
        .await;
    assert!(delete.is_err());
    policies::check(&pool, &app, actor, role, &entries).await;
    waits::check(&pool, &app, actor, &entries).await;
    iam::check(&pool, &app).await;
}
