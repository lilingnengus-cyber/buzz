use super::*;
use crate::test_fixture::adjustments as fixture;
use crate::test_fixture::seed;
use business_core::PgStore;
fn context(actor: Uuid, capability: &str) -> RequestContext {
    RequestContext {
        enterprise_user_id: actor,
        identity_binding_id: Uuid::new_v4(),
        delegation_id: Uuid::new_v4(),
        agent_id: "adjustment-test".into(),
        agent_turn_id: "adjustment-turn".into(),
        trace_id: Uuid::new_v4(),
        used_calls: 1,
        required_scope: capability.into(),
        source_buzz_event_id: Uuid::new_v4().simple().to_string().repeat(2),
        source_channel_id: "isolated-adjustment-channel".into(),
    }
}
fn grant(c: &RequestContext, legal: Uuid) -> EffectiveGrant {
    EffectiveGrant {
        capability: business_iam::Capability::parse(&c.required_scope).unwrap(),
        data_scope: DataScope::Restricted(BTreeMap::from([(
            "legal_entity".into(),
            [legal.to_string()].into(),
        )])),
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

#[tokio::test]
async fn reads_real_core_without_writes_and_enforces_delegated_scope() {
    let Ok(url) = std::env::var("BUSINESS_ADJUSTMENT_READ_TEST_DATABASE_URL") else {
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
    let order = fixture::source(&pool, &f).await;
    let batch = fixture::draft(&pool, &f, order, "read-adapter-draft").await;
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,'profit_adjustment:read' FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).execute(&pool).await.unwrap();
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
    let c = context(f.actor, "profit_adjustment:read");
    let scope = iam_authorization_scope(&grant(&c, f.legal_entity), &c.required_scope).unwrap();
    let before:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM business_core_audit_events),(SELECT count(*) FROM profit_facts),(SELECT count(*) FROM operational_adjustment_previews)").fetch_one(&pool).await.unwrap();
    let search = value(
        read(
            &core,
            "search_operational_adjustments",
            &json!({"limit":1}),
            &scope,
            &c,
        )
        .await,
    )
    .await;
    assert_eq!(search["items"].as_array().unwrap().len(), 1);
    assert_eq!(search["items"][0]["id"], json!(batch));
    let input = json!({"documentId":batch});
    let detail = value(read(&core, "get_operational_adjustment", &input, &scope, &c).await).await;
    assert_eq!(detail["items"][0]["version"], 1);
    assert_eq!(detail["items"][0]["lines"][0]["amount"], "10.010000");
    assert_eq!(
        detail["items"][0]["lines"][0]["directSalesOrderId"],
        json!(order)
    );
    assert_eq!(detail["resourceRefs"], json!([]));
    for key in [
        "legal_entity",
        "customer",
        "business_unit",
        "brand",
        "warehouse",
        "supplier",
    ] {
        let mut g = grant(&c, f.legal_entity);
        if let DataScope::Restricted(dims) = &mut g.data_scope {
            dims.insert(key.into(), [Uuid::new_v4().to_string()].into());
        }
        let denied = iam_authorization_scope(&g, &c.required_scope).unwrap();
        assert_eq!(
            read(&core, "get_operational_adjustment", &input, &denied, &c)
                .await
                .status(),
            StatusCode::FORBIDDEN,
            "{key}"
        );
    }
    assert_eq!(
        read(
            &core,
            "get_operational_adjustment",
            &json!({"documentId":batch,"offset":1}),
            &scope,
            &c
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        read(
            &core,
            "get_operational_adjustment",
            &json!({"documentId":batch,"expectedVersion":2}),
            &scope,
            &c
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let mut brand_scope = scope.clone();
    brand_scope.brand_ids.insert(f.brand.to_string());
    assert_eq!(
        read(
            &core,
            "get_operational_adjustment",
            &input,
            &brand_scope,
            &c
        )
        .await
        .status(),
        StatusCode::OK
    );
    sqlx::query("UPDATE sales_orders SET brand_id=NULL WHERE id=$1")
        .bind(order)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        read(
            &core,
            "get_operational_adjustment",
            &input,
            &brand_scope,
            &c
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let after:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM business_core_audit_events),(SELECT count(*) FROM profit_facts),(SELECT count(*) FROM operational_adjustment_previews)").fetch_one(&pool).await.unwrap();
    assert_eq!(before, after);
    if let Ok(path) = std::env::var("BUSINESS_ADJUSTMENT_READ_PROOF") {
        std::fs::write(
            path,
            json!({"search":search,"detail":detail,"traceId":c.trace_id}).to_string(),
        )
        .unwrap();
    }
    server.abort();
}
