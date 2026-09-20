use super::*;
use crate::test_fixture::seed;
use business_core::PgStore;
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
fn report_inputs_are_strict() {
    let input = json!({"reportType":"management_profit_statement","managementPeriod":"2026-08","currency":"CNY"});
    assert!(valid("prepare_management_report_snapshot", &input));
    let mut bad = input.clone();
    bad["operation"] = json!("approve");
    assert!(!valid("prepare_management_report_snapshot", &bad));
    bad = input;
    bad["reportType"] = json!("profitability_by_dimension");
    assert!(!valid("prepare_management_report_snapshot", &bad));
    let mut approval = json!({"documentId":Uuid::new_v4(),"expectedVersion":1,"previewHash":"a".repeat(64),"decision":"approve"});
    assert!(valid("approve_management_report_snapshot", &approval));
    approval["sourceBuzzEventId"] = json!("a".repeat(64));
    assert!(!valid("approve_management_report_snapshot", &approval));
}
#[tokio::test]
async fn report_adapter_checks_scope_before_prepare_and_confirm() {
    let Ok(url) = std::env::var("BUSINESS_REPORT_ADAPTER_TEST_DATABASE_URL") else {
        return;
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(12)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let f = seed(&pool).await;
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,'management_report:generate_snapshot' FROM business_user_roles WHERE enterprise_user_id=$1").bind(f.actor).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES('management_report:generate_snapshot','management_report:generate_snapshot',ARRAY['b2_operator'],1,true)").execute(&pool).await.unwrap();
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
    let tool = "prepare_management_report_snapshot";
    let c = context(f.actor, required_capability(tool).unwrap());
    let allowed = grant(&c, f.legal_entity, f.business_unit, f.customer);
    let input = json!({"reportType":"management_profit_statement","managementPeriod":"2026-08","currency":"CNY","legalEntityIds":[f.legal_entity]});
    for dimension in [
        "legal_entity",
        "customer",
        "business_unit",
        "brand",
        "warehouse",
        "supplier",
    ] {
        let mut denied = allowed.clone();
        if let DataScope::Restricted(dims) = &mut denied.data_scope {
            dims.insert(dimension.into(), [Uuid::new_v4().to_string()].into());
        }
        assert_eq!(
            forward(&core, tool, input.clone(), &c, &denied)
                .await
                .status(),
            StatusCode::FORBIDDEN,
            "{dimension}"
        );
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM business_agent_report_snapshot_intents")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    let prepared = value(forward(&core, tool, input.clone(), &c, &allowed).await).await;
    assert_eq!(prepared["resourceRefs"], json!([]));
    let mut tampered = prepared["document"].clone();
    tampered["sourceHash"] = json!("a".repeat(64));
    assert!(!valid_snapshot(
        &tampered,
        "management_report_snapshot_intent"
    ));
    tampered = prepared["document"].clone();
    tampered["scope"]["legalEntityIds"] = json!(["bad"]);
    assert!(!valid_snapshot(
        &tampered,
        "management_report_snapshot_intent"
    ));
    let tool = "approve_management_report_snapshot";
    let c = context(f.actor, required_capability(tool).unwrap());
    let allowed = grant(&c, f.legal_entity, f.business_unit, f.customer);
    let approval = json!({"documentId":prepared["item"]["id"],"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve"});
    let denied = grant(&c, f.legal_entity, f.business_unit, Uuid::new_v4());
    assert_eq!(
        forward(&core, tool, approval.clone(), &c, &denied)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM management_report_snapshots")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let result = value(forward(&core, tool, approval, &c, &allowed).await).await;
    assert_eq!(result["executed"], true);
    assert_eq!(
        result["resourceRefs"][0]["bizUri"],
        format!(
            "biz://management-report/{}",
            result["createdDocument"]["id"].as_str().unwrap()
        )
    );
    assert_eq!(result["preview"], prepared["document"]);
    let tool = "prepare_management_report_snapshot";
    let c = context(f.actor, required_capability(tool).unwrap());
    let allowed = grant(&c, f.legal_entity, f.business_unit, f.customer);
    let reused = value(forward(&core, tool, input, &c, &allowed).await).await;
    assert_eq!(
        reused["document"]["effects"]["createsImmutableSnapshot"],
        false
    );
    assert_eq!(reused["resourceRefs"], result["resourceRefs"]);
    let tool = "approve_management_report_snapshot";
    let c = context(f.actor, required_capability(tool).unwrap());
    let allowed = grant(&c, f.legal_entity, f.business_unit, f.customer);
    let approval = json!({"documentId":reused["item"]["id"],"expectedVersion":1,"previewHash":reused["previewHash"],"decision":"approve"});
    let replay = value(forward(&core, tool, approval, &c, &allowed).await).await;
    assert_eq!(
        replay["createdDocument"]["id"],
        result["createdDocument"]["id"]
    );
    assert_eq!(replay["resourceRefs"], result["resourceRefs"]);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM management_report_snapshots")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    server.abort();
}
