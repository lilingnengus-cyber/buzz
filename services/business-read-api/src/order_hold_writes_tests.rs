use super::*;
use crate::test_fixture::{seed, Fixture};
use business_core::{
    b2::{
        model::{
            CreateInventoryOpening, CreateSalesOrder, DecimalString, InventoryOpeningLineInput,
            SalesOrderLineInput, VersionCommand,
        },
        InventoryService, SalesService,
    },
    PgStore,
};
use chrono::NaiveDate;
fn context(actor: Uuid, capability: &str) -> RequestContext {
    RequestContext {
        enterprise_user_id: actor,
        identity_binding_id: Uuid::new_v4(),
        delegation_id: Uuid::new_v4(),
        agent_id: "order-hold-test".into(),
        agent_turn_id: "order-hold-turn".into(),
        trace_id: Uuid::new_v4(),
        used_calls: 1,
        required_scope: capability.into(),
        source_buzz_event_id: Uuid::new_v4().simple().to_string().repeat(2),
        source_channel_id: "isolated-order-hold-channel".into(),
    }
}
fn grant(c: &RequestContext, legal: Uuid, unit: Uuid, customer: Uuid) -> EffectiveGrant {
    EffectiveGrant {
        capability: business_iam::Capability::parse(&c.required_scope).unwrap(),
        data_scope: DataScope::Restricted(BTreeMap::from([
            ("legal_entity".into(), [legal.to_string()].into()),
            ("business_unit".into(), [unit.to_string()].into()),
            ("customer".into(), [customer.to_string()].into()),
        ])),
        obligations: Default::default(),
    }
}
async fn value(response: Response) -> Value {
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 131072)
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status, StatusCode::OK, "{value}");
    value
}
#[test]
fn hold_inputs_bind_the_fixed_operation_and_only_signed_confirmation_fields() {
    let id = Uuid::new_v4();
    for operation in ["hold", "release_hold"] {
        let tool = format!("prepare_sales_order_{operation}");
        let mut input = json!({"sourceDocumentId":id,"expectedSourceVersion":2,"reason":"Review"});
        assert!(valid(&tool, &input));
        input["operation"] = json!("release_hold");
        assert!(!valid(&tool, &input));
        input.as_object_mut().unwrap().remove("operation");
        input["reason"] = json!("   ");
        assert!(!valid(&tool, &input));
        let tool = format!("approve_sales_order_{operation}");
        let mut input = json!({"documentId":id,"expectedVersion":1,"previewHash":"a".repeat(64),"decision":"approve"});
        assert!(valid(&tool, &input));
        input["sourceBuzzEventId"] = json!("a".repeat(64));
        assert!(!valid(&tool, &input));
    }
}
fn export(tool: &str, trace: Uuid, result: &Value) {
    use std::io::Write;
    if let Ok(path) = std::env::var("BUSINESS_ORDER_HOLD_MCP_FIXTURE_FILE") {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(
            file,
            "{}",
            json!({"tool":tool,"traceId":trace,"result":result})
        )
        .unwrap();
    }
}
#[tokio::test]
async fn hold_adapter_checks_every_order_line_before_persisting_and_before_approval() {
    let Ok(url) = std::env::var("BUSINESS_ORDER_HOLD_ADAPTER_TEST_DATABASE_URL") else {
        return;
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(12)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let fixture = seed(&pool).await;
    let actor = fixture.actor;
    let warehouse_two = Uuid::new_v4();
    sqlx::query("INSERT INTO business_warehouses(id,legal_entity_id,business_unit_id,code,name) VALUES($1,$2,$3,'HOLD_WH_TWO','Second warehouse')").bind(warehouse_two).bind(fixture.legal_entity).bind(fixture.business_unit).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_warehouse_scopes(enterprise_user_id,warehouse_id,granted_by) VALUES($1,$2,$1)").bind(actor).bind(warehouse_two).execute(&pool).await.unwrap();

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
                lines: [fixture.warehouse, warehouse_two]
                    .map(|warehouse_id| InventoryOpeningLineInput {
                        warehouse_id,
                        sku_id: fixture.sku,
                        quantity: dec(10),
                        unit_cost: dec(5),
                    })
                    .to_vec(),
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
    let created = create_order(
        &sales,
        &fixture,
        date,
        "hold-adapter-fixture",
        warehouse_two,
    )
    .await;
    sales
        .confirm_order(
            actor,
            Uuid::new_v4(),
            created.id,
            "hold-adapter-confirm",
            &version(1),
        )
        .await
        .unwrap();
    sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES('sales_order:place_hold','sales_order:place_hold',ARRAY['b2_operator'],1,true),('sales_order:release_hold','sales_order:release_hold',ARRAY['b2_operator'],1,true)").execute(&pool).await.unwrap();
    let config = business_core::Config::from_env().unwrap();
    let router = business_core::router(business_core::AppState::new(store, &config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let core = CoreClient {
        client: reqwest::Client::new(),
        base_url: Url::parse(&format!("http://{address}/")).unwrap(),
        credential: config.service_credential,
    };
    let tool = "prepare_sales_order_hold";
    let c = context(actor, required_capability(tool).unwrap());
    let stale_input =
        json!({"sourceDocumentId":created.id,"expectedSourceVersion":2,"reason":"RACE"});
    let dry = fetch(
        &core,
        "v1/agent-order-hold-previews/sales_order_hold_intent",
        Some(&stale_input),
        &c,
        tool,
    )
    .await
    .unwrap();
    let hash: String = Sha256::digest(serde_json::to_vec(&dry["document"]).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    sqlx::query("UPDATE sales_orders SET business_note='changed between scope check and prepare' WHERE id=$1").bind(created.id).execute(&pool).await.unwrap();
    let rejected = fetch_bound(
        &core,
        "v1/agent-order-hold-intents/sales_order_hold_intent",
        Some(&stale_input),
        &c,
        tool,
        Some(&hash),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(rejected.status(), StatusCode::CONFLICT);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM business_agent_order_hold_intents")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    for op in ["hold", "release_hold"] {
        let version: i64 = sqlx::query_scalar("SELECT version FROM sales_orders WHERE id=$1")
            .bind(created.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        let tool = format!("prepare_sales_order_{op}");
        let c = context(actor, required_capability(&tool).unwrap());
        let mut allowed = grant(
            &c,
            fixture.legal_entity,
            fixture.business_unit,
            fixture.customer,
        );
        if let DataScope::Restricted(dims) = &mut allowed.data_scope {
            dims.insert(
                "warehouse".into(),
                [fixture.warehouse.to_string(), warehouse_two.to_string()].into(),
            );
            dims.insert("brand".into(), [fixture.brand.to_string()].into());
        }
        let input = json!({"sourceDocumentId":created.id,"expectedSourceVersion":version,"reason":"REVIEW"});
        let mut denied = allowed.clone();
        if let DataScope::Restricted(dims) = &mut denied.data_scope {
            dims.insert("warehouse".into(), [fixture.warehouse.to_string()].into());
        }
        let before: i64 =
            sqlx::query_scalar("SELECT count(*) FROM business_agent_order_hold_intents")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            forward(&core, &tool, input.clone(), &c, &denied)
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM business_agent_order_hold_intents")
                .fetch_one(&pool)
                .await
                .unwrap(),
            before
        );
        let rejected = fetch_bound(
            &core,
            &format!("v1/agent-order-hold-intents/{}", family(&tool).unwrap()),
            Some(&input),
            &c,
            &tool,
            Some(&"0".repeat(64)),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(rejected.status(), StatusCode::CONFLICT);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM business_agent_order_hold_intents")
                .fetch_one(&pool)
                .await
                .unwrap(),
            before
        );
        let prepared = value(forward(&core, &tool, input, &c, &allowed).await).await;
        assert_eq!(
            prepared["resourceRefs"][0]["bizUri"],
            format!("biz://sales-order/{}", created.id)
        );
        let scope = iam_authorization_scope(&allowed, &c.required_scope).unwrap();
        let kind = family(&tool).unwrap();
        let snapshot = &prepared["document"];
        assert_eq!(snapshot["lines"].as_array().unwrap().len(), 2);
        assert!(permits(snapshot, &scope, kind));
        let mut substituted = snapshot.clone();
        substituted["lines"][0]["warehouseId"] = json!(Uuid::new_v4());
        assert!(!permits(&substituted, &scope, kind));
        for (field, replacement) in [
            ("operation", json!("other")),
            ("targetHoldStatus", json!("other")),
            ("canExecute", json!(false)),
            ("changesInventoryReservation", json!(true)),
        ] {
            let mut changed = snapshot.clone();
            changed[field] = replacement;
            assert!(!valid_snapshot(&changed, kind));
        }
        export(&tool, c.trace_id, &prepared);
        let tool = format!("approve_sales_order_{op}");
        let c = context(actor, required_capability(&tool).unwrap());
        allowed.capability = business_iam::Capability::parse(&c.required_scope).unwrap();
        denied.capability = allowed.capability.clone();
        let input = json!({"documentId":prepared["item"]["id"],"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve"});
        assert_eq!(
            forward(&core, &tool, input.clone(), &c, &denied)
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        let mut wrong = input.clone();
        wrong["previewHash"] = json!("0".repeat(64));
        assert_eq!(
            forward(&core, &tool, wrong, &c, &allowed).await.status(),
            StatusCode::CONFLICT
        );
        let result = value(forward(&core, &tool, input, &c, &allowed).await).await;
        assert_eq!(result["executed"], true);
        assert_eq!(result["createdDocument"]["version"], version + 1);
        export(&tool, c.trace_id, &result);
    }
    server.abort();
}
async fn create_order(
    sales: &SalesService,
    fixture: &Fixture,
    date: NaiveDate,
    key: &str,
    warehouse_two: Uuid,
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
                lines: [fixture.warehouse, warehouse_two]
                    .map(|warehouse_id| SalesOrderLineInput {
                        sku_id: fixture.sku,
                        warehouse_id,
                        unit_of_measure_id: fixture.uom,
                        quantity: dec(4),
                        unit_price: dec(100),
                        discount_amount: dec(0),
                        tax_rate: dec(0),
                        business_unit_id: None,
                        department_id: None,
                        brand_id: Some(fixture.brand),
                    })
                    .to_vec(),
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
