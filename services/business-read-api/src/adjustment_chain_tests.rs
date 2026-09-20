//! Signed Gateway -> real stdio MCP -> Read API -> Core acceptance on an isolated database.
use super::*;
use crate::test_fixture::adjustments as fixture;
use crate::test_fixture::seed;
use business_auth_gateway::{
    auth::{Audience, Claims, JwtVerifier},
    model::{ChallengeRequest, RequestFacts},
    Store,
};
use business_core::PgStore;
use chrono::Utc;
use nostr::{EventBuilder, Keys, Kind, Tag, TagKind};
use sqlx::PgPool;
mod gateway;
mod mcp;

async fn serve(app: Router) -> (Url, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, task)
}
async fn issue(url: &Url, credential: &str, keys: &Keys, content: &str, scope: &str) -> Value {
    let channel = Uuid::new_v4().to_string();
    let trace = Uuid::new_v4();
    let turn = Uuid::new_v4().to_string();
    let event = EventBuilder::new(Kind::TextNote, content)
        .tags([Tag::custom(TagKind::Custom("h".into()), [channel.clone()])])
        .sign_with_keys(keys)
        .unwrap();
    let response=reqwest::Client::new().post(url.join("internal/agent-delegations").unwrap()).header("x-business-service-credential",credential).header("x-trace-id",trace.to_string()).json(&json!({"sourceEvent":event,"sourceBuzzEventId":event.id.to_hex(),"sourceBuzzPubkey":event.pubkey.to_hex(),"sourceChannelId":channel,"agentId":"adjustment-chain-agent","agentTurnId":turn,"scopes":[scope]})).send().await.unwrap();
    let code = response.status();
    let mut result: Value = response.json().await.unwrap();
    assert_eq!(code, StatusCode::OK, "delegation issuance failed");
    result["turnId"] = json!(turn);
    result["sourceEventId"] = json!(event.id.to_hex());
    result
}
#[tokio::test]
async fn signed_adjustment_runs_through_real_gateway_mcp_api_and_core() {
    let Ok(url) = std::env::var("BUSINESS_ADJUSTMENT_CHAIN_TEST_DATABASE_URL") else {
        return;
    };
    let binary = std::env::var("BUSINESS_ADJUSTMENT_CHAIN_MCP_BINARY")
        .expect("build and supply the actual MCP binary");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(16)
        .connect(&url)
        .await
        .unwrap();
    let core_store = PgStore::new(pool.clone());
    core_store.migrate().await.unwrap();
    let f = seed(&pool).await;
    let source = fixture::source(&pool, &f).await;
    let core_config = business_core::Config::from_env().unwrap();
    let credential = core_config.service_credential.clone();
    let mut config = gateway::config(url, 8);
    config.business_read_service_credential = Some(credential.clone());
    let store = Store::new(pool.clone(), config.clone());
    let claims = Claims {
        iss: "https://issuer.test".into(),
        sub: f.actor.to_string(),
        exp: Utc::now().timestamp() + 3600,
        aud: Some(Audience::One("workbench".into())),
        azp: Some("workbench".into()),
        client_id: None,
        email: None,
        name: Some("Adjustment chain actor".into()),
        preferred_username: None,
        sid: Some("adjustment-chain-session".into()),
        events: None,
    };
    let principal = store
        .principal(&claims, &gateway::facts(Uuid::new_v4()))
        .await
        .unwrap();
    assert_eq!(principal.user_id, f.actor);
    let keys = Keys::generate();
    let challenge = store
        .challenge(
            &principal,
            ChallengeRequest {
                pubkey: keys.public_key().to_hex(),
            },
            gateway::facts(Uuid::new_v4()),
        )
        .await
        .unwrap();
    let event = EventBuilder::new(Kind::Custom(24243), challenge.payload)
        .sign_with_keys(&keys)
        .unwrap();
    store
        .verify_binding(
            &principal,
            challenge.id,
            event,
            gateway::facts(Uuid::new_v4()),
        )
        .await
        .unwrap();
    let human = Uuid::new_v4();
    sqlx::query("INSERT INTO business_iam.principals(id,kind,external_id,display_name) VALUES($1,'human',$2,'Adjustment Chain')").bind(human).bind(f.actor.to_string()).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_iam.principal_permissions(principal_id,permission_id) SELECT $1,id FROM business_iam.permissions WHERE capability IN ('operational_adjustment_post_intent:create','operational_adjustment_post_intent:approve')").bind(human).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES('profit_adjustment:post','profit_adjustment:post',ARRAY['b2_operator'],1,true)").execute(&pool).await.unwrap();
    let (gateway_url, gateway_task) = serve(business_auth_gateway::router(
        business_auth_gateway::AppState {
            store,
            verifier: JwtVerifier::new(&config),
            config,
        },
    ))
    .await;
    let (core_url, core_task) = serve(business_core::router(business_core::AppState::new(
        core_store,
        &core_config,
    )))
    .await;
    let (api_url, api_task) = serve(
        router_with_runtime(
            credential.clone(),
            DelegationVerifier::Gateway {
                client: reqwest::Client::new(),
                url: gateway_url.clone(),
                credential: credential.clone(),
            },
            RouterRuntime {
                rule_config: RuleConfig::bundled().unwrap(),
                max_findings: 100,
                max_payload_bytes: 131072,
                core: Some(CoreClient {
                    client: reqwest::Client::new(),
                    base_url: core_url,
                    credential: credential.clone(),
                }),
                draft_write_enabled: true,
                chat_approval_enabled: true,
            },
        )
        .unwrap(),
    )
    .await;
    for decision in ["approve", "reject"] {
        let batch = fixture::draft(&pool, &f, source, &format!("chain-{decision}-draft")).await;
        let issued = issue(
            &gateway_url,
            &credential,
            &keys,
            "准备指定费用批次过账",
            "operational_adjustment_post_intent:create",
        )
        .await;
        let mut client =
            mcp::Client::start(&binary, &gateway_url, &api_url, &credential, &issued, None).await;
        let prepared = client
            .call(
                "prepare_operational_adjustment_post",
                json!({"batchId":batch,"expectedVersion":1}),
            )
            .await;
        assert_eq!(prepared["status"], "ok", "{prepared}");
        client.stop().await;
        assert_eq!(batch_status(&pool, batch).await, "draft");
        let command = prepared[if decision == "approve" {
            "approvalCommand"
        } else {
            "rejectionCommand"
        }]
        .as_str()
        .unwrap();
        let issued = issue(
            &gateway_url,
            &credential,
            &keys,
            command,
            "operational_adjustment_post_intent:approve",
        )
        .await;
        let mut client = mcp::Client::start(
            &binary,
            &gateway_url,
            &api_url,
            &credential,
            &issued,
            Some("operational_adjustment_post_intent:approve"),
        )
        .await;
        let result = client
            .call("approve_operational_adjustment_post", json!({}))
            .await;
        assert_eq!(result["executed"], decision == "approve", "{result}");
        assert_eq!(
            result["status"],
            if decision == "approve" {
                "executed"
            } else {
                "rejected"
            }
        );
        let repeat = client
            .call("approve_operational_adjustment_post", json!({}))
            .await;
        assert_ne!(
            repeat["executed"], true,
            "duplicate signed command must not execute"
        );
        client.stop().await;
        assert_eq!(
            batch_status(&pool, batch).await,
            if decision == "approve" {
                "posted"
            } else {
                "draft"
            }
        );
        let source_id: String = sqlx::query_scalar(
            "SELECT source_buzz_event_id FROM business_document_approval_votes WHERE request_id=$1",
        )
        .bind(
            result["requestId"]
                .as_str()
                .unwrap()
                .parse::<Uuid>()
                .unwrap(),
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(source_id, issued["sourceEventId"].as_str().unwrap());
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM profit_facts WHERE source_type='operational_adjustment' AND source_id=$1").bind(batch).fetch_one(&pool).await.unwrap();
        assert_eq!(count, i64::from(decision == "approve"));
    }
    for mode in ["revoked", "wrong_hash", "stale_version"] {
        let batch = fixture::draft(&pool, &f, source, &format!("chain-negative-{mode}")).await;
        let issued = issue(
            &gateway_url,
            &credential,
            &keys,
            "准备费用批次过账",
            "operational_adjustment_post_intent:create",
        )
        .await;
        let mut client =
            mcp::Client::start(&binary, &gateway_url, &api_url, &credential, &issued, None).await;
        let prepared = client
            .call(
                "prepare_operational_adjustment_post",
                json!({"batchId":batch,"expectedVersion":1}),
            )
            .await;
        assert_eq!(prepared["status"], "ok");
        client.stop().await;
        let mut command = prepared["approvalCommand"].as_str().unwrap().to_owned();
        if mode == "wrong_hash" {
            command = command.replace(prepared["previewHash"].as_str().unwrap(), &"0".repeat(64));
        }
        let issued = issue(
            &gateway_url,
            &credential,
            &keys,
            &command,
            "operational_adjustment_post_intent:approve",
        )
        .await;
        if mode == "revoked" {
            let response = reqwest::Client::new()
                .post(
                    gateway_url
                        .join(&format!(
                            "internal/agent-delegations/{}/revoke",
                            issued["id"].as_str().unwrap()
                        ))
                        .unwrap(),
                )
                .header("x-business-service-credential", &credential)
                .header("x-trace-id", issued["traceId"].as_str().unwrap())
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
        }
        if mode == "stale_version" {
            sqlx::query("UPDATE operational_adjustment_batches SET version=version+1 WHERE id=$1")
                .bind(batch)
                .execute(&pool)
                .await
                .unwrap();
        }
        let mut client = mcp::Client::start(
            &binary,
            &gateway_url,
            &api_url,
            &credential,
            &issued,
            Some("operational_adjustment_post_intent:approve"),
        )
        .await;
        let result = client
            .call("approve_operational_adjustment_post", json!({}))
            .await;
        assert_ne!(result["executed"], true, "{mode}");
        assert_ne!(result["status"], "executed", "{mode}");
        client.stop().await;
        assert_eq!(batch_status(&pool, batch).await, "draft");
        let facts:i64=sqlx::query_scalar("SELECT count(*) FROM profit_facts WHERE source_type='operational_adjustment' AND source_id=$1").bind(batch).fetch_one(&pool).await.unwrap();
        assert_eq!(facts, 0, "{mode}");
        let requests:i64=sqlx::query_scalar("SELECT count(*) FROM business_document_approval_requests WHERE document_type='operational_adjustment_post_intent' AND document_id=$1").bind(prepared["item"]["id"].as_str().unwrap().parse::<Uuid>().unwrap()).fetch_one(&pool).await.unwrap();
        assert_eq!(requests, 0, "{mode}");
    }
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM security_audit_events WHERE event_type='BUSINESS_MCP_TOOL_SUCCEEDED'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 7);
    api_task.abort();
    core_task.abort();
    gateway_task.abort();
}
async fn batch_status(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM operational_adjustment_batches WHERE id=$1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}
