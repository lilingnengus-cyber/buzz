#[path = "adjustment_writes_draft_tests.rs"]
mod draft_cases;
#[path = "adjustment_writes_reversal_tests.rs"]
mod reversal_cases;
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

#[test]
fn strict_inputs_and_catalog() {
    let mut input = json!({"batchId":Uuid::new_v4(),"expectedVersion":1});
    assert!(valid("prepare_operational_adjustment_post", &input));
    input["amount"] = json!("1");
    assert!(!valid("prepare_operational_adjustment_post", &input));
    let mut approval = json!({"documentId":Uuid::new_v4(),"expectedVersion":1,"previewHash":"a".repeat(64),"decision":"approve"});
    assert!(valid("approve_operational_adjustment_post", &approval));
    approval["sourceBuzzEventId"] = json!("a".repeat(64));
    assert!(!valid("approve_operational_adjustment_post", &approval));
    assert!(!is_approval_tool("prepare_operational_adjustment_post"));
    assert!(is_approval_tool("approve_operational_adjustment_post"));
    for tool in [
        "prepare_operational_adjustment_post",
        "approve_operational_adjustment_post",
    ] {
        assert!(WRITE_TOOLS.contains(&tool));
        assert!(required_capability(tool)
            .unwrap()
            .starts_with("operational_adjustment_post_intent:"));
    }
}
#[tokio::test]
async fn adjustment_adapter_binds_real_core_preview_and_posting() {
    let Ok(url) = std::env::var("BUSINESS_ADJUSTMENT_ADAPTER_TEST_DATABASE_URL") else {
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
    sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES('profit_adjustment:post','profit_adjustment:post',ARRAY['b2_operator'],1,true)").execute(&pool).await.unwrap();
    let config = business_core::Config::from_env().unwrap();
    let app = business_core::router(business_core::AppState::new(store, &config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let core = CoreClient {
        client: reqwest::Client::new(),
        base_url: Url::parse(&format!("http://{address}/")).unwrap(),
        credential: config.service_credential,
    };
    draft_cases::verify(&pool, &core, &f, order).await;
    for (decision, minimum) in [("approve", 1), ("reject", 1), ("approve", 2)] {
        sqlx::query("UPDATE business_approval_policies SET min_approvers=$1 WHERE action_code='profit_adjustment:post'").bind(minimum as i16).execute(&pool).await.unwrap();
        let batch = fixture::draft(
            &pool,
            &f,
            order,
            &format!("adapter-draft-{decision}-{minimum}"),
        )
        .await;
        let input = json!({"batchId":batch,"expectedVersion":1});
        let tool = "prepare_operational_adjustment_post";
        let c = context(f.actor, required_capability(tool).unwrap());
        let allowed = grant(&c, f.legal_entity);
        let before: i64 =
            sqlx::query_scalar("SELECT count(*) FROM business_agent_adjustment_intents")
                .fetch_one(&pool)
                .await
                .unwrap();
        for dimension in [
            "legal_entity",
            "customer",
            "business_unit",
            "brand",
            "warehouse",
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
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM business_agent_adjustment_intents")
                .fetch_one(&pool)
                .await
                .unwrap(),
            before
        );
        let prepared = value(forward(&core, tool, input.clone(), &c, &allowed).await).await;
        assert!(valid_snapshot(
            &prepared["document"],
            "operational_adjustment_post_intent"
        ));
        let replay = value(forward(&core, tool, input, &c, &allowed).await).await;
        assert_eq!(replay["item"]["id"], prepared["item"]["id"]);
        for path in [
            "/allocationPreview/preview/totalAmount",
            "/allocationPreview/preview/targets/0/customerId",
            "/allocationPreview/preview/allocations/0/targets/0/amount",
        ] {
            let mut bad = prepared["document"].clone();
            *bad.pointer_mut(path).unwrap() = json!("0");
            assert!(!valid_snapshot(&bad, "operational_adjustment_post_intent"));
            // Re-sign the inner payload to test semantic consistency, not only hashing.
            let bytes = serde_json::to_vec(&bad["allocationPreview"]["preview"]).unwrap();
            bad["allocationPreview"]["previewHash"] = json!(Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>());
            assert!(!valid_snapshot(&bad, "operational_adjustment_post_intent"));
        }
        let tool = "approve_operational_adjustment_post";
        let c = context(f.actor, required_capability(tool).unwrap());
        let allowed = grant(&c, f.legal_entity);
        let input = json!({"documentId":prepared["item"]["id"],"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":decision});
        let mut wrong = input.clone();
        wrong["previewHash"] = json!("0".repeat(64));
        assert_eq!(
            forward(&core, tool, wrong, &c, &allowed).await.status(),
            StatusCode::CONFLICT
        );
        let result = value(forward(&core, tool, input, &c, &allowed).await).await;
        assert_eq!(result["executed"], decision == "approve" && minimum == 1);
        assert_eq!(result["preview"], prepared["document"]);
        if let Ok(path) = std::env::var("BUSINESS_ADJUSTMENT_ADAPTER_PROOF") {
            use std::io::Write;
            let mut output = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap();
            writeln!(output, "{}", json!({"prepared":prepared,"approval":result,"decision":decision,"minimumApprovers":minimum})).unwrap();
        }
        if decision == "approve" && minimum == 1 {
            assert_eq!(result["postedDocument"]["id"], json!(batch));
            assert_eq!(result["postedDocument"]["version"], 3);
        } else {
            assert!(result["postedDocument"].is_null());
        }
        let status: String =
            sqlx::query_scalar("SELECT status FROM operational_adjustment_batches WHERE id=$1")
                .bind(batch)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            status,
            if decision == "approve" && minimum == 1 {
                "posted"
            } else {
                "draft"
            }
        );
    }
    reversal_cases::verify(&pool, &core, &f, order).await;
    server.abort();
}
