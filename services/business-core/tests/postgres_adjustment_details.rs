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
#[path = "support/adjustment_intent_fixture.rs"]
mod fixture;
#[tokio::test]
async fn details_check_all_lines_current_and_historical_scope_and_versions() {
    let Ok(url) = std::env::var("BUSINESS_CORE_ADJUSTMENT_DETAIL_TEST_DATABASE_URL") else {
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
    let order = fixture::source(&pool, &f).await;
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,'profit_adjustment:read' FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).execute(&pool).await.unwrap();
    let batch = fixture::draft(&pool, &f, order, "detail-source-draft").await;
    let second_brand = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO business_brands(id,code,name) VALUES($1,'DETAIL_BRAND','Detail Brand')",
    )
    .bind(second_brand)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(second_brand).execute(&pool).await.unwrap();
    // A second line is outside page one, but must still participate in authorization.
    sqlx::query("INSERT INTO operational_adjustment_lines(id,batch_id,line_number,metric_type,amount,currency,business_date,management_period,legal_entity_id,direct_sales_order_id,brand_id,allocation_basis,allocation_scope,reason_code) SELECT $1,batch_id,2,metric_type,20.02,currency,business_date,management_period,legal_entity_id,direct_sales_order_id,$2,allocation_basis,allocation_scope,reason_code FROM operational_adjustment_lines WHERE batch_id=$3").bind(Uuid::new_v4()).bind(second_brand).bind(batch).execute(&pool).await.unwrap();
    sqlx::query("UPDATE sales_orders SET brand_id=NULL WHERE id=$1")
        .bind(order)
        .execute(&pool)
        .await
        .unwrap();
    let config = Config::from_env().unwrap();
    let app = business_core::router(AppState::new(store.clone(), &config));
    let path = format!("/v1/profit-adjustments/{batch}?limit=1");
    let before: (i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM operational_adjustment_previews),(SELECT count(*) FROM business_core_audit_events),(SELECT count(*) FROM profit_facts)").fetch_one(&pool).await.unwrap();
    let (status, first) = call(&app, f.actor, "GET", &path, Value::Null, "").await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["lines"].as_array().unwrap().len(), 1);
    assert_eq!(first["pagination"]["total"], 2);
    assert_eq!(first["pagination"]["nextOffset"], 1);
    assert_eq!(
        first["totalAmount"]
            .as_str()
            .unwrap()
            .parse::<rust_decimal::Decimal>()
            .unwrap(),
        rust_decimal::Decimal::new(3003, 2)
    );
    let second = format!("/v1/profit-adjustments/{batch}?offset=1&limit=1&expectedVersion=1");
    let (status, page) = call(&app, f.actor, "GET", &second, Value::Null, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["lines"][0]["line_number"], 2);
    assert!(page["pagination"]["nextOffset"].is_null());
    assert_eq!(
        call(
            &app,
            f.actor,
            "GET",
            &format!("/v1/profit-adjustments/{batch}?offset=1"),
            Value::Null,
            ""
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    sqlx::query("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2")
        .bind(f.actor)
        .bind(second_brand)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        call(&app, f.actor, "GET", &path, Value::Null, "").await.0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(second_brand).execute(&pool).await.unwrap();
    sqlx::query("UPDATE operational_adjustment_batches SET version=version+1 WHERE id=$1")
        .bind(batch)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        call(&app, f.actor, "GET", &second, Value::Null, "").await.0,
        StatusCode::CONFLICT
    );
    let after:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM operational_adjustment_previews),(SELECT count(*) FROM business_core_audit_events),(SELECT count(*) FROM profit_facts)").fetch_one(&pool).await.unwrap();
    assert_eq!(before, after);
    // Frozen historical customer dimensions remain protected even if today's order moved.
    let posted = fixture::draft(&pool, &f, order, "detail-posted-draft").await;
    let service = business_core::b4::AdjustmentService::new(store, "ADJ".into(), 500);
    let version = business_core::b4::model::VersionCommand {
        expected_version: 1,
    };
    let preview = service
        .allocation_preview(f.actor, posted, &version)
        .await
        .unwrap();
    service
        .post_guarded(
            f.actor,
            Uuid::new_v4(),
            posted,
            "detail-guarded-post",
            &version,
            &preview,
        )
        .await
        .unwrap();
    let posted_path = format!("/v1/profit-adjustments/{posted}");
    assert_eq!(
        call(&app, f.actor, "GET", &posted_path, Value::Null, "")
            .await
            .0,
        StatusCode::OK
    );
    let other = Uuid::new_v4();
    sqlx::query("INSERT INTO business_customers(id,legal_entity_id,business_unit_id,code,name,credit_currency) VALUES($1,$2,$3,'DETAIL_OTHER','Other Customer','CNY')").bind(other).bind(f.legal_entity).bind(f.business_unit).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(other).execute(&pool).await.unwrap();
    sqlx::query("UPDATE sales_orders SET customer_id=$2 WHERE id=$1")
        .bind(order)
        .bind(other)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2",
    )
    .bind(f.actor)
    .bind(f.customer)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        call(&app, f.actor, "GET", &posted_path, Value::Null, "")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let mut disabled = config;
    disabled.operational_adjustments_enabled = false;
    let disabled = business_core::router(AppState::new(PgStore::new(pool), &disabled));
    assert_eq!(
        call(&disabled, f.actor, "GET", &posted_path, Value::Null, "")
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
}
