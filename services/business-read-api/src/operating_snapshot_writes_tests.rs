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

#[test]
fn strict_inputs() {
    let mut input = json!({"cadence":"weekly","periodStart":"2026-01-19","currency":"CNY","utcOffsetMinutes":480});
    assert!(valid("prepare_operating_report_snapshot", &input));
    input["periodStart"] = json!("2026-01-20");
    assert!(!valid("prepare_operating_report_snapshot", &input));
    input["cadence"] = json!("daily");
    input["operation"] = json!("approve");
    assert!(!valid("prepare_operating_report_snapshot", &input));
    let mut approval = json!({"documentId":Uuid::new_v4(),"expectedVersion":1,"previewHash":"a".repeat(64),"decision":"approve"});
    assert!(valid("approve_operating_report_snapshot", &approval));
    approval["sourceBuzzEventId"] = json!("a".repeat(64));
    assert!(!valid("approve_operating_report_snapshot", &approval));
}
#[tokio::test]
async fn operating_adapter_uses_real_core_and_checks_before_writing() {
    let Ok(url) = std::env::var("BUSINESS_OPERATING_ADAPTER_TEST_DATABASE_URL") else {
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
    for permission in [
        "management_report:generate_snapshot",
        "management_report:read",
    ] {
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,$2 FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).bind(permission).execute(&pool).await.unwrap();
    }
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
    for cadence in ["daily", "weekly"] {
        let input = json!({"cadence":cadence,"periodStart":"2026-01-19","currency":"CNY","utcOffsetMinutes":480});
        let tool = "prepare_operating_report_snapshot";
        let c = context(f.actor, required_capability(tool).unwrap());
        let allowed = grant(&c, f.legal_entity);
        let before: i64 =
            sqlx::query_scalar("SELECT count(*) FROM business_agent_operating_snapshot_intents")
                .fetch_one(&pool)
                .await
                .unwrap();
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
        let after: i64 =
            sqlx::query_scalar("SELECT count(*) FROM business_agent_operating_snapshot_intents")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(before, after);
        let prepared = value(forward(&core, tool, input.clone(), &c, &allowed).await).await;
        export(tool, c.trace_id, &prepared);
        assert_eq!(prepared["document"]["ownerUserId"], json!(f.actor));
        assert_eq!(prepared["resourceRefs"], json!([]));
        for field in ["sourceHash", "scopeHash", "periodEndUtc", "ownerUserId"] {
            let mut bad = prepared["document"].clone();
            bad[field] = json!("bad");
            assert!(
                !valid_snapshot(&bad, "operating_report_snapshot_intent"),
                "{field}"
            );
        }
        let tool = "approve_operating_report_snapshot";
        let c = context(f.actor, required_capability(tool).unwrap());
        let allowed = grant(&c, f.legal_entity);
        let command = json!({"documentId":prepared["item"]["id"],"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve"});
        let denied = grant(&c, Uuid::new_v4());
        assert_eq!(
            forward(&core, tool, command.clone(), &c, &denied)
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        let done = value(forward(&core, tool, command, &c, &allowed).await).await;
        export(tool, c.trace_id, &done);
        assert_eq!(done["executed"], true);
        assert_eq!(done["createdDocument"]["ownerUserId"], json!(f.actor));
        assert_eq!(
            done["createdDocument"]["sourceHash"],
            prepared["document"]["sourceHash"]
        );
        // A new intent for frozen content must reuse the original snapshot.
        let tool = "prepare_operating_report_snapshot";
        let c = context(f.actor, required_capability(tool).unwrap());
        let prepared =
            value(forward(&core, tool, input, &c, &grant(&c, f.legal_entity)).await).await;
        assert_eq!(
            prepared["document"]["existingSnapshot"]["id"],
            done["createdDocument"]["id"]
        );
        let tool = "approve_operating_report_snapshot";
        let c = context(f.actor, required_capability(tool).unwrap());
        let command = json!({"documentId":prepared["item"]["id"],"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve"});
        let reused =
            value(forward(&core, tool, command, &c, &grant(&c, f.legal_entity)).await).await;
        assert_eq!(
            reused["createdDocument"]["id"],
            done["createdDocument"]["id"]
        );
        export(tool, c.trace_id, &reused);
        assert_eq!(reused["createdDocument"]["created"], false);
    }
    sqlx::query("UPDATE business_approval_policies SET min_approvers=2 WHERE action_code='management_report:generate_snapshot'").execute(&pool).await.unwrap();
    for decision in ["approve", "reject"] {
        let tool = "prepare_operating_report_snapshot";
        let c = context(f.actor, required_capability(tool).unwrap());
        let prepared=value(forward(&core,tool,json!({"cadence":"daily","periodStart":"2026-02-02","currency":"CNY","utcOffsetMinutes":480}),&c,&grant(&c,f.legal_entity)).await).await;
        let tool = "approve_operating_report_snapshot";
        let c = context(f.actor, required_capability(tool).unwrap());
        let input = json!({"documentId":prepared["item"]["id"],"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":decision});
        let result = value(forward(&core, tool, input, &c, &grant(&c, f.legal_entity)).await).await;
        assert_eq!(result["executed"], false);
        assert!(result["createdDocument"].is_null());
        assert_eq!(
            result["status"],
            if decision == "approve" {
                "pending"
            } else {
                "rejected"
            }
        );
        export(tool, c.trace_id, &result);
    }
    server.abort();
}

fn export(tool: &str, trace: Uuid, result: &Value) {
    use std::io::Write;
    if let Ok(path) = std::env::var("BUSINESS_OPERATING_MCP_FIXTURE_FILE") {
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
