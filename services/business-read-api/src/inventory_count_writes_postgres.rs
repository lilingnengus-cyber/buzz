use super::*;
use business_core::PgStore;

use crate::test_fixture::{seed, Fixture};

fn config(database_url: String, service_credential: String) -> business_core::Config {
    business_core::Config {
        database_url,
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        service_credential,
        service_audience: "business-core".into(),
        bootstrap_enabled: false,
        bootstrap_user_id: None,
        sales_enabled: true,
        inventory_enabled: true,
        receivables_enabled: true,
        purchasing_enabled: true,
        receiving_enabled: true,
        payables_enabled: true,
        profitability_enabled: true,
        management_reporting_enabled: true,
        operational_adjustments_enabled: true,
        profit_projection_worker_enabled: true,
        profit_projection_batch_size: 200,
        profit_projection_retry_limit: 5,
        sales_order_number_prefix: "SO".into(),
        shipment_number_prefix: "SHP".into(),
        receivable_number_prefix: "AR".into(),
        customer_receipt_number_prefix: "RCPT".into(),
        inventory_opening_number_prefix: "OPEN".into(),
        inventory_count_number_prefix: "CNT".into(),
        purchase_requisition_number_prefix: "PRQ".into(),
        purchase_order_number_prefix: "PO".into(),
        goods_receipt_number_prefix: "GR".into(),
        trade_payable_number_prefix: "AP".into(),
        supplier_payment_number_prefix: "PAY".into(),
        sales_return_number_prefix: "SRET".into(),
        purchase_return_number_prefix: "PRET".into(),
        profit_adjustment_number_prefix: "ADJ".into(),
        management_report_snapshot_number_prefix: "MGR".into(),
        profit_management_timezone: "Asia/Shanghai".into(),
        profit_default_currency: "CNY".into(),
        profit_allocation_max_targets: 500,
        profit_report_max_rows: 1000,
        profit_data_stale_after_minutes: 15,
        default_payment_terms_days: 30,
        default_supplier_payment_terms_days: 30,
        default_currency: "CNY".into(),
        command_rate_limit_per_minute: 60,
        business_web_origin: "https://business.example.test".into(),
        business_web_embed_origin: "https://business.example.test".into(),
        business_session_cookie_name: "__Host-bizfin_business".into(),
    }
}

async fn value(response: Response) -> Value {
    let status = response.status();
    let body: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}
fn authority(ctx: &RequestContext, f: &Fixture) -> EffectiveGrant {
    EffectiveGrant {
        capability: business_iam::Capability::parse(&ctx.required_scope).unwrap(),
        data_scope: DataScope::Restricted(BTreeMap::from([
            ("legal_entity".into(), [f.legal_entity.to_string()].into()),
            ("warehouse".into(), [f.warehouse.to_string()].into()),
            ("business_unit".into(), [f.business_unit.to_string()].into()),
            ("brand".into(), [f.brand.to_string()].into()),
        ])),
        obligations: Default::default(),
    }
}
async fn prepare(core: &CoreClient, f: &Fixture, name: &str, input: Value) -> Value {
    let tool = format!("prepare_inventory_count_{name}");
    let mut ctx = context(required_capability(&tool).unwrap());
    ctx.enterprise_user_id = f.actor;
    value(forward(core, &tool, input, &ctx, &authority(&ctx, f)).await).await
}
async fn approve(
    core: &CoreClient,
    f: &Fixture,
    name: &str,
    prepared: &Value,
    decision: &str,
) -> Value {
    let tool = format!("approve_inventory_count_{name}");
    let mut ctx = context(required_capability(&tool).unwrap());
    ctx.enterprise_user_id = f.actor;
    ctx.source_buzz_event_id = Uuid::new_v4().simple().to_string().repeat(2);
    value(forward(core,&tool,json!({"documentId":prepared["item"]["id"],"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":decision}),&ctx,&authority(&ctx,f)).await).await
}
async fn balance(store: &PgStore, f: &Fixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('quantity',on_hand_quantity::text,'value',inventory_value::text) FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3")
        .bind(f.legal_entity).bind(f.warehouse).bind(f.sku).fetch_one(store.pool()).await.unwrap()
}
#[tokio::test]
async fn real_core_preparation_and_approval_complete_count_lifecycle() {
    let Ok(url) = std::env::var("BUSINESS_COUNT_WORKFLOW_DATABASE_URL") else {
        eprintln!("BUSINESS_COUNT_WORKFLOW_DATABASE_URL absent; isolated count workflow skipped");
        return;
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(12)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool);
    store.migrate().await.unwrap();
    let f = seed(store.pool()).await;
    sqlx::query(
        "INSERT INTO inventory_balances(legal_entity_id,warehouse_id,sku_id) VALUES($1,$2,$3)",
    )
    .bind(f.legal_entity)
    .bind(f.warehouse)
    .bind(f.sku)
    .execute(store.pool())
    .await
    .unwrap();
    for action in [
        "inventory_opening:create",
        "inventory_opening:post",
        "inventory_opening:reverse",
    ] {
        sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval,require_distinct_business_unit) VALUES($1,$1,ARRAY['b2_operator'],1,true,false)").bind(action).execute(store.pool()).await.unwrap();
    }
    let core_router = business_core::api::router(business_core::api::AppState::new(
        store.clone(),
        &config(url, "count-test-credential".into()),
    ));
    let (core, task) = serve(core_router).await;
    let create = json!({"legalEntityId":f.legal_entity,"warehouseId":f.warehouse,"countDate":"2026-09-20","currency":"CNY","skuIds":[f.sku]});
    let prepared = prepare(&core, &f, "creation", create.clone()).await;
    assert_eq!(prepared["resourceRefs"], json!([]));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM inventory_count_tasks")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
    let created = approve(&core, &f, "creation", &prepared, "approve").await;
    assert_eq!(created["executed"], true);
    let id: Uuid = created["createdDocument"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        created["resourceRefs"][0]["bizUri"],
        format!("biz://inventory-count/{id}")
    );
    let service = business_core::b2::InventoryCountService::new(store.clone(), "IC".into());
    let detail = service.detail(f.actor, id).await.unwrap();
    assert_eq!(detail.status, "counting");
    assert_eq!(balance(&store, &f).await["quantity"], "0.000000");
    let submitted=prepare(&core,&f,"submission",json!({"inventoryCountId":id,"command":{"expectedVersion":1,"lines":[{"countLineId":detail.lines[0].id,"actualOnHandQuantity":"2","surplusUnitCost":"7"}]}})).await;
    let entered = approve(&core, &f, "submission", &submitted, "approve").await;
    assert_eq!(entered["updatedDocument"]["status"], "counted");
    assert_eq!(balance(&store, &f).await["quantity"], "0.000000");
    let posting = prepare(
        &core,
        &f,
        "posting",
        json!({"inventoryCountId":id,"command":{"expectedVersion":2}}),
    )
    .await;
    assert_eq!(balance(&store, &f).await["quantity"], "0.000000");
    let posted = approve(&core, &f, "posting", &posting, "approve").await;
    assert_eq!(posted["updatedDocument"]["status"], "posted");
    assert_eq!(
        balance(&store, &f).await,
        json!({"quantity":"2.000000","value":"14.000000"})
    );
    let prepared = prepare(&core, &f, "creation", create.clone()).await;
    let created = approve(&core, &f, "creation", &prepared, "approve").await;
    let second = created["createdDocument"]["id"].clone();
    let cancel = prepare(
        &core,
        &f,
        "cancellation",
        json!({"inventoryCountId":second,"command":{"expectedVersion":1,"reasonCode":"改期盘点"}}),
    )
    .await;
    let cancelled = approve(&core, &f, "cancellation", &cancel, "approve").await;
    assert_eq!(cancelled["updatedDocument"]["status"], "cancelled");
    assert_eq!(
        balance(&store, &f).await,
        json!({"quantity":"2.000000","value":"14.000000"})
    );
    let rejected = prepare(&core, &f, "creation", create).await;
    let result = approve(&core, &f, "creation", &rejected, "reject").await;
    assert_eq!(result["status"], "rejected");
    assert_eq!(result["executed"], false);
    assert_eq!(result["resourceRefs"], json!([]));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM inventory_count_tasks")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 2);
    let freezes: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM inventory_count_tasks WHERE status IN ('counting','counted')",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(freezes, 0);
    task.abort();
}

#[path = "inventory_count_capacity.rs"]
mod capacity;
