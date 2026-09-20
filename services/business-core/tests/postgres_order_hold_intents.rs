use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use business_core::{
    b2::{
        model::{
            CreateInventoryOpening, CreateSalesOrder, DecimalString, InventoryOpeningLineInput,
            SalesOrderLineInput, VersionCommand,
        },
        InventoryService, SalesService,
    },
    AppState, Config, PgStore,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
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
    json!({"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve","sourceBuzzEventId":Uuid::new_v4().simple().to_string().repeat(2),"sourceChannelId":"isolated-order-hold-test"})
}
fn approval_path(kind: &str, prepared: &Value) -> String {
    format!(
        "/v1/agent-approvals/order-holds/{kind}/{}",
        prepared["item"]["id"].as_str().unwrap()
    )
}
async fn prepare(app: &Router, actor: Uuid, kind: &str, input: Value) -> Value {
    let key = Uuid::new_v4().to_string();
    let path = format!("/v1/agent-order-hold-intents/{kind}");
    let (status, result) = call(app, actor, "POST", &path, input.clone(), &key).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let (status, replay) = call(app, actor, "POST", &path, input, &key).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["item"]["id"], result["item"]["id"]);
    result
}
#[tokio::test]
async fn order_hold_intents_are_atomic_and_bound_to_current_authority() {
    let Ok(url) = std::env::var("BUSINESS_CORE_ORDER_HOLD_TEST_DATABASE_URL") else {
        eprintln!("skipping: isolated order hold database unset");
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let fixture = b2_seed::seed(&pool).await;
    let actor = fixture.actor;
    let sales = SalesService::new(store.clone(), "SO".into(), "SHP".into(), 30);
    let inventory = InventoryService::new(store.clone(), "OPEN".into(), "AR".into());
    let date = NaiveDate::from_ymd_opt(2026, 9, 20).unwrap();
    let opening = inventory
        .create_opening(
            fixture.actor,
            Uuid::new_v4(),
            "opening-create-0001",
            &CreateInventoryOpening {
                legal_entity_id: fixture.legal_entity,
                business_date: date,
                currency: "CNY".into(),
                lines: vec![InventoryOpeningLineInput {
                    warehouse_id: fixture.warehouse,
                    sku_id: fixture.sku,
                    quantity: dec(10),
                    unit_cost: dec(5),
                }],
            },
        )
        .await
        .unwrap();
    let posted = inventory
        .post_opening(
            fixture.actor,
            Uuid::new_v4(),
            opening.id,
            "opening-post-0001",
            &version(1),
        )
        .await
        .unwrap();
    assert_eq!(posted.status, "posted");
    let created = create_order(&sales, &fixture, date, "order-hold-fixture").await;
    sales
        .confirm_order(
            actor,
            Uuid::new_v4(),
            created.id,
            "order-hold-confirm",
            &version(1),
        )
        .await
        .unwrap();
    let config = Config::from_env().unwrap();
    let app = business_core::router(AppState::new(store, &config));
    let kind = "sales_order_hold_intent";
    let input =
        json!({"sourceDocumentId":created.id,"expectedSourceVersion":2,"reason":"CREDIT_REVIEW"});
    let (code, dry) = call(
        &app,
        actor,
        "POST",
        &format!("/v1/agent-order-hold-previews/{kind}"),
        input.clone(),
        "",
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{dry}");
    assert_eq!(dry["document"]["canExecute"], true);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM business_agent_order_hold_intents")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    let mut unknown = input.clone();
    unknown["operation"] = json!("release");
    assert_eq!(
        call(
            &app,
            actor,
            "POST",
            &format!("/v1/agent-order-hold-intents/{kind}"),
            unknown,
            "hold-unknown-input"
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let prepared = prepare(&app, actor, kind, input.clone()).await;
    let intent_id: Uuid = prepared["item"]["id"].as_str().unwrap().parse().unwrap();
    let key: String = sqlx::query_scalar(
        "SELECT idempotency_key FROM business_agent_order_hold_intents WHERE id=$1",
    )
    .bind(intent_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut changed = input.clone();
    changed["reason"] = json!("OTHER_REASON");
    assert_eq!(
        call(
            &app,
            actor,
            "POST",
            &format!("/v1/agent-order-hold-intents/{kind}"),
            changed,
            &key
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert!(
        sqlx::query("UPDATE business_agent_order_hold_intents SET input='{}' WHERE id=$1")
            .bind(intent_id)
            .execute(&pool)
            .await
            .is_err()
    );
    assert_eq!(
        call(
            &app,
            actor,
            "POST",
            &approval_path(kind, &prepared),
            vote(&prepared),
            ""
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(counts(&pool).await, (0, 0));
    sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES('sales_order:place_hold','sales_order:place_hold',ARRAY['b2_operator'],1,true),('sales_order:release_hold','sales_order:release_hold',ARRAY['b2_operator'],1,true)").execute(&pool).await.unwrap();
    let mut wrong = vote(&prepared);
    wrong["reason"] = json!("OTHER");
    assert_eq!(
        call(
            &app,
            actor,
            "POST",
            &approval_path(kind, &prepared),
            wrong,
            ""
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    sqlx::query("UPDATE sales_orders SET business_note='changed after preview' WHERE id=$1")
        .bind(created.id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            actor,
            "POST",
            &approval_path(kind, &prepared),
            vote(&prepared),
            ""
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(counts(&pool).await, (0, 0));
    for (kind, target_status) in [
        ("sales_order_hold_intent", "manual_review_hold"),
        ("sales_order_release_hold_intent", "none"),
    ] {
        let current: i64 = sqlx::query_scalar("SELECT version FROM sales_orders WHERE id=$1")
            .bind(created.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        let prepared=prepare(&app,actor,kind,json!({"sourceDocumentId":created.id,"expectedSourceVersion":current,"reason":"REVIEW"})).await;
        let (code, result) = call(
            &app,
            actor,
            "POST",
            &approval_path(kind, &prepared),
            vote(&prepared),
            "",
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{result}");
        assert_eq!(result["executed"], true);
        assert_eq!(result["createdDocument"]["id"], json!(created.id));
        assert_eq!(
            call(
                &app,
                actor,
                "POST",
                &approval_path(kind, &prepared),
                vote(&prepared),
                ""
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        let actual: (String, i64) =
            sqlx::query_as("SELECT hold_status,version FROM sales_orders WHERE id=$1")
                .bind(created.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(actual, (target_status.into(), current + 1));
    }
    let inventory_state:(Decimal,Decimal)=sqlx::query_as("SELECT on_hand_quantity,reserved_quantity FROM inventory_balances WHERE warehouse_id=$1 AND sku_id=$2").bind(fixture.warehouse).bind(fixture.sku).fetch_one(&pool).await.unwrap();
    assert_eq!(inventory_state, (Decimal::from(10), Decimal::from(8)));
    expiry_during_wait(&pool, &app, actor, created.id).await;
    current_voters(&pool, &app, &fixture, created.id).await;
    let current: i64 = sqlx::query_scalar("SELECT version FROM sales_orders WHERE id=$1")
        .bind(created.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let kind = "sales_order_release_hold_intent";
    let prepared=prepare(&app,actor,kind,json!({"sourceDocumentId":created.id,"expectedSourceVersion":current,"reason":"REJECTED_REVIEW"})).await;
    let mut rejection = vote(&prepared);
    rejection["decision"] = json!("reject");
    let (code, result) = call(
        &app,
        actor,
        "POST",
        &approval_path(kind, &prepared),
        rejection,
        "",
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], false);
    assert_eq!(result["status"], "rejected");
    assert_eq!(
        call(
            &app,
            actor,
            "POST",
            &approval_path(kind, &prepared),
            vote(&prepared),
            ""
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let actual: (String, i64) =
        sqlx::query_as("SELECT hold_status,version FROM sales_orders WHERE id=$1")
            .bind(created.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(actual, ("manual_review_hold".into(), current));
}
async fn counts(pool: &PgPool) -> (i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM business_document_approval_requests),(SELECT count(*) FROM business_document_approval_votes)").fetch_one(pool).await.unwrap()
}
async fn expiry_during_wait(pool: &PgPool, app: &Router, actor: Uuid, order: Uuid) {
    let version: i64 = sqlx::query_scalar("SELECT version FROM sales_orders WHERE id=$1")
        .bind(order)
        .fetch_one(pool)
        .await
        .unwrap();
    let kind = "sales_order_hold_intent";
    let original = prepare(
        app,
        actor,
        kind,
        json!({"sourceDocumentId":order,"expectedSourceVersion":version,"reason":"EXPIRY"}),
    )
    .await;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO business_agent_order_hold_intents(id,kind,input,snapshot,created_by_user_id,idempotency_key,trace_id,expires_at) SELECT $1,kind,input,snapshot,created_by_user_id,$1::text,trace_id,clock_timestamp()+interval '2 seconds' FROM business_agent_order_hold_intents WHERE id=$2")
        .bind(id).bind(original["item"]["id"].as_str().unwrap().parse::<Uuid>().unwrap()).execute(pool).await.unwrap();
    let mut prepared = original;
    prepared["item"]["id"] = json!(id);
    let before = counts(pool).await;
    let mut blocker = pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM sales_orders WHERE id=$1 FOR UPDATE")
        .bind(order)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let app = app.clone();
    let pending = tokio::spawn(async move {
        call(
            &app,
            actor,
            "POST",
            &approval_path(kind, &prepared),
            vote(&prepared),
            "",
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(pid)
            .fetch_one(pool)
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    sqlx::query("SELECT pg_sleep(GREATEST(EXTRACT(EPOCH FROM (expires_at-clock_timestamp()))::double precision,0)+0.1) FROM business_agent_order_hold_intents WHERE id=$1").bind(id).execute(pool).await.unwrap();
    blocker.commit().await.unwrap();
    assert_ne!(pending.await.unwrap().0, StatusCode::OK);
    assert_eq!(counts(pool).await, before);
    let actual: (String, i64) =
        sqlx::query_as("SELECT hold_status,version FROM sales_orders WHERE id=$1")
            .bind(order)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(actual, ("none".into(), version));
}
async fn create_order(
    sales: &SalesService,
    fixture: &Fixture,
    date: NaiveDate,
    key: &str,
) -> business_core::b2::model::CommandResult {
    sales
        .create_order(
            fixture.actor,
            Uuid::new_v4(),
            key,
            &CreateSalesOrder {
                legal_entity_id: fixture.legal_entity,
                customer_id: fixture.customer,
                salesperson_user_id: None,
                business_unit_id: fixture.business_unit,
                department_id: None,
                brand_id: Some(fixture.brand),
                currency: "CNY".into(),
                order_date: date,
                requested_delivery_date: Some(date),
                payment_terms_days: None,
                customer_reference: None,
                business_note: None,
                lines: vec![SalesOrderLineInput {
                    sku_id: fixture.sku,
                    warehouse_id: fixture.warehouse,
                    unit_of_measure_id: fixture.uom,
                    quantity: dec(8),
                    unit_price: dec(100),
                    discount_amount: dec(0),
                    tax_rate: dec(0),
                    business_unit_id: None,
                    department_id: None,
                    brand_id: Some(fixture.brand),
                }],
            },
        )
        .await
        .unwrap()
}

fn dec(value: i64) -> DecimalString {
    DecimalString(Decimal::from(value))
}

fn version(expected_version: i64) -> VersionCommand {
    VersionCommand {
        expected_version,
        reason_code: None,
    }
}

async fn current_voters(pool: &PgPool, app: &Router, f: &Fixture, order: Uuid) {
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    for voter in [first, second] {
        sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'order-hold-test',$1::text,'Reviewer')").bind(voter).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) SELECT $1,role_id,$2 FROM business_user_roles WHERE enterprise_user_id=$2").bind(voter).bind(f.actor).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO business_legal_entity_scopes(enterprise_user_id,legal_entity_id,granted_by) VALUES($1,$2,$3)").bind(voter).bind(f.legal_entity).bind(f.actor).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$3)").bind(voter).bind(f.business_unit).bind(f.actor).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$3)").bind(voter).bind(f.customer).bind(f.actor).execute(pool).await.unwrap();
    }
    let version: i64 = sqlx::query_scalar("SELECT version FROM sales_orders WHERE id=$1")
        .bind(order)
        .fetch_one(pool)
        .await
        .unwrap();
    let kind = "sales_order_hold_intent";
    let prepared = prepare(
        app,
        f.actor,
        kind,
        json!({"sourceDocumentId":order,"expectedSourceVersion":version,"reason":"TWO_REVIEWERS"}),
    )
    .await;
    let path = approval_path(kind, &prepared);
    sqlx::query("UPDATE business_approval_policies SET min_approvers=2,allow_self_approval=false WHERE action_code='sales_order:place_hold'").execute(pool).await.unwrap();
    let before = counts(pool).await;
    assert_eq!(
        call(app, f.actor, "POST", &path, vote(&prepared), "")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(counts(pool).await, before);
    let (code, pending) = call(app, first, "POST", &path, vote(&prepared), "").await;
    assert_eq!(code, StatusCode::OK, "{pending}");
    assert_eq!(pending["executed"], false);
    assert_eq!(pending["approvalCount"], 1);
    // Weakening the policy cannot reduce a pending request's original threshold.
    sqlx::query("UPDATE business_approval_policies SET min_approvers=1 WHERE action_code='sales_order:place_hold'").execute(pool).await.unwrap();
    let pending_counts = counts(pool).await;
    for revoked in [f.actor, first] {
        sqlx::query(
            "DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2",
        )
        .bind(revoked)
        .bind(f.customer)
        .execute(pool)
        .await
        .unwrap();
        assert_eq!(
            call(app, second, "POST", &path, vote(&prepared), "")
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(counts(pool).await, pending_counts);
        let actual: (String, i64) =
            sqlx::query_as("SELECT hold_status,version FROM sales_orders WHERE id=$1")
                .bind(order)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(actual, ("none".into(), version));
        sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$3)").bind(revoked).bind(f.customer).bind(f.actor).execute(pool).await.unwrap();
    }
    let (code, result) = call(app, second, "POST", &path, vote(&prepared), "").await;
    assert_eq!(code, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true);
    assert_eq!(result["minimumApprovers"], 2);
    assert_eq!(result["approvalCount"], 2);
}
