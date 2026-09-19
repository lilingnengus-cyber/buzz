use super::*;
use business_core::PgStore;
use sqlx::PgPool;
async fn seed(pool: &PgPool) -> (Uuid, Uuid, Uuid, Uuid) {
    let actor = Uuid::new_v4();
    let outsider = Uuid::new_v4();
    let legal = Uuid::new_v4();
    let unit = Uuid::new_v4();
    let customer = Uuid::new_v4();
    let role = Uuid::new_v4();
    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'crm-test',$1::text,'CRM User'),($2,'crm-test',$2::text,'Other User')").bind(actor).bind(outsider).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_roles(id,role_key,name) VALUES($1,'crm_test','CRM Test')")
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,'crm:read'),($1,'crm:manage')").bind(role).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) VALUES($1,$3,$1),($2,$3,$1)").bind(actor).bind(outsider).bind(role).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_legal_entities(id,code,name,country_code,functional_currency) VALUES($1,'CRM_LE','CRM LE','CN','CNY')").bind(legal).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_units(id,legal_entity_id,code,name) VALUES($1,$2,'CRM_BU','CRM BU')",
    )
    .bind(unit)
    .bind(legal)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_customers(id,legal_entity_id,business_unit_id,code,name,credit_currency) VALUES($1,$2,$3,'CRM_C','CRM Customer','CNY')").bind(customer).bind(legal).bind(unit).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_legal_entity_scopes(enterprise_user_id,legal_entity_id,granted_by) VALUES($1,$2,$1)").bind(actor).bind(legal).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)").bind(actor).bind(unit).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)").bind(actor).bind(customer).execute(pool).await.unwrap();
    (actor, legal, unit, customer)
}
fn context(actor: Uuid, capability: &str) -> RequestContext {
    RequestContext {
        enterprise_user_id: actor,
        identity_binding_id: Uuid::new_v4(),
        delegation_id: Uuid::new_v4(),
        agent_id: "crm-test".into(),
        agent_turn_id: "crm-turn".into(),
        trace_id: Uuid::new_v4(),
        used_calls: 1,
        required_scope: capability.into(),
        source_buzz_event_id: Uuid::new_v4().simple().to_string().repeat(2),
        source_channel_id: "isolated-crm-channel".into(),
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
fn fixed_crm_inputs_cannot_select_other_operations_or_confirmation_sources() {
    let id = Uuid::new_v4();
    let fields = json!({"legalEntityId":id,"businessUnitId":id,"title":"Opportunity","companyName":"Company","stage":"new","currency":"CNY"});
    assert!(valid("prepare_crm_creation", &fields));
    let mut bad = fields.clone();
    bad["expectedVersion"] = 1.into();
    assert!(!valid("prepare_crm_creation", &bad));
    bad = fields.clone();
    bad["operation"] = "followup".into();
    assert!(!valid("prepare_crm_creation", &bad));
    assert!(!valid(
        "prepare_crm_update",
        &json!({"opportunityId":id,"command":fields})
    ));
    for kind in ["creation", "update", "followup"] {
        let tool = format!("approve_crm_{kind}");
        let mut input = json!({"documentId":id,"expectedVersion":1,"previewHash":"a".repeat(64),"decision":"approve"});
        assert!(valid(&tool, &input));
        input["sourceBuzzEventId"] = "b".repeat(64).into();
        assert!(!valid(&tool, &input));
        assert_eq!(
            required_capability(&tool),
            Some(format!("crm_{kind}_intent:approve").as_str())
        );
    }
}
#[tokio::test]
async fn crm_adapter_real_core_closes_three_operations_and_denies_before_persistence() {
    let Ok(url) = std::env::var("BUSINESS_CRM_ADAPTER_TEST_DATABASE_URL") else {
        eprintln!("skipping: BUSINESS_CRM_ADAPTER_TEST_DATABASE_URL unset");
        return;
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let (actor, legal, unit, customer) = seed(&pool).await;
    sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES('crm:manage','crm:manage',ARRAY['crm_test'],1,true)").execute(&pool).await.unwrap();
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
    let fields = json!({"legalEntityId":legal,"businessUnitId":unit,"customerId":customer,"title":"Assistant CRM","companyName":"Scoped Company","stage":"new","currency":"CNY","expectedAmountMinor":20000});
    let c = context(actor, "crm_creation_intent:create");
    let denied = grant(&c, legal, unit, Uuid::new_v4());
    assert_eq!(
        forward(&core, "prepare_crm_creation", fields.clone(), &c, &denied)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM business_agent_crm_intents")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let mut id = Uuid::nil();
    for (n, name) in ["creation", "update", "followup"].into_iter().enumerate() {
        let tool = format!("prepare_crm_{name}");
        let kind = format!("crm_{name}_intent");
        let input = match n {
            0 => fields.clone(),
            1 => {
                let mut command = fields.clone();
                command["stage"] = "quoting".into();
                command["expectedVersion"] = 1.into();
                json!({"opportunityId":id,"command":command})
            }
            _ => {
                json!({"opportunityId":id,"command":{"note":"Human supplied follow-up","stage":"won","nextAction":"Prepare order","expectedVersion":2}})
            }
        };
        let c = context(actor, &format!("{kind}:create"));
        let prepared = value(
            forward(
                &core,
                &tool,
                input.clone(),
                &c,
                &grant(&c, legal, unit, customer),
            )
            .await,
        )
        .await;
        assert_eq!(prepared["documentType"], kind);
        assert!(prepared["item"].get("snapshot").is_none());
        assert_eq!(
            prepared["resourceRefs"].as_array().unwrap().len(),
            if n == 0 { 0 } else { 1 }
        );
        let again = value(
            forward(
                &core,
                &tool,
                input.clone(),
                &c,
                &grant(&c, legal, unit, customer),
            )
            .await,
        )
        .await;
        assert_eq!(again["item"]["id"], prepared["item"]["id"]);
        let approve = format!("approve_crm_{name}");
        let c = context(actor, &format!("{kind}:approve"));
        let approval = json!({"documentId":prepared["item"]["id"],"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve"});
        let mut bad = approval.clone();
        bad["previewHash"] = "0".repeat(64).into();
        assert_eq!(
            forward(&core, &approve, bad, &c, &grant(&c, legal, unit, customer))
                .await
                .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            forward(
                &core,
                &approve,
                approval.clone(),
                &c,
                &grant(&c, legal, unit, Uuid::new_v4())
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        let result = value(
            forward(
                &core,
                &approve,
                approval.clone(),
                &c,
                &grant(&c, legal, unit, customer),
            )
            .await,
        )
        .await;
        assert_eq!(result["executed"], true);
        assert_eq!(result["createdDocument"]["version"], (n + 1) as i64);
        id = serde_json::from_value(result["createdDocument"]["id"].clone()).unwrap();
        assert_eq!(
            result["resourceRefs"][0]["bizUri"],
            format!("biz://crm-opportunity/{id}")
        );
        assert!(forward(
            &core,
            &approve,
            approval,
            &c,
            &grant(&c, legal, unit, customer)
        )
        .await
        .status()
        .is_client_error());
    }
    let row: (String, i64) =
        sqlx::query_as("SELECT stage,version FROM crm_opportunities WHERE id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row, ("won".into(), 3));
    let totals:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM crm_opportunities),(SELECT count(*) FROM crm_followups),(SELECT count(*) FROM sales_orders)").fetch_one(&pool).await.unwrap();
    assert_eq!(totals, (1, 1, 0));
    // Clearing a customer must not avoid the original target's delegated scope.
    let mut command = fields;
    command["expectedVersion"] = 3.into();
    command["customerId"] = Value::Null;
    let c = context(actor, "crm_update_intent:create");
    assert_eq!(
        forward(
            &core,
            "prepare_crm_update",
            json!({"opportunityId":id,"command":command}),
            &c,
            &grant(&c, legal, unit, Uuid::new_v4())
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM business_agent_crm_intents")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 3);
    server.abort();
}
