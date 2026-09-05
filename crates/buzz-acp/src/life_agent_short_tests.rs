use super::*;
use axum::{extract::State, routing::post, Json, Router};
use nostr::{EventBuilder, Keys, Kind, Tag};
use serde_json::{json, Value};

#[tokio::test]
async fn short_confirmation_preserves_signature_and_issues_only_bound_execute() {
    let command = Uuid::new_v4();
    let trace = Uuid::new_v4();
    async fn resolve(
        State((command, _)): State<(Uuid, Uuid)>,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        let signed: Event =
            serde_json::from_value(body["signedEvent"].clone()).expect("signed event");
        assert!(signed.verify_signature());
        assert_eq!(signed.content, "确认删除");
        assert_eq!(body["communityId"], "community");
        Json(json!({"commandId":command,"expectedVersion":7,"previewHash":"a".repeat(64)}))
    }
    async fn issue(
        State((command, trace)): State<(Uuid, Uuid)>,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        assert_eq!(body["sourceEvent"]["content"], "确认删除");
        assert_eq!(body["writeCommandId"], command.to_string());
        assert_eq!(
            body["requestedCapabilities"],
            json!(["write_command:execute"])
        );
        assert_eq!(body["resourceContext"]["expectedVersion"], 7);
        assert_eq!(body["resourceContext"]["previewHash"], "a".repeat(64));
        Json(
            json!({"delegationId":Uuid::new_v4(),"token":"d".repeat(43),"audience":"life-workbench-mcp",
            "effectiveCapabilities":["write_command:execute"],"maxCalls":1,"traceId":trace}),
        )
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listen");
    let url = Url::parse(&format!(
        "http://{}",
        listener.local_addr().expect("address")
    ))
    .expect("url");
    let router = Router::new()
        .route("/v1/write-confirmations/confirm-delete", post(resolve))
        .route("/v1/life-agent/delegations", post(issue))
        .with_state((command, trace));
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("server");
    });
    let mut config = LifeAgentHostConfig::test_mock();
    config.gateway_base_url = url;
    let channel = Uuid::new_v4();
    let source = EventBuilder::new(Kind::Custom(9), "确认删除")
        .tag(Tag::parse(vec!["h".into(), channel.to_string()]).expect("channel"))
        .sign_with_keys(&Keys::generate())
        .expect("sign");
    let agent = Keys::generate().public_key().to_hex();
    let participants = vec![source.pubkey.to_hex(), agent.clone()];
    let access = config
        .authorize_turn(LifeAuthorizationRequest {
            source_event: &source,
            source_channel_id: channel,
            community_id: "community",
            participant_pubkeys: &participants,
            direct_message: true,
            agent_id: &agent,
            agent_turn_id: "short-confirm",
            trace_id: &trace.to_string(),
        })
        .await
        .expect("access");
    assert_eq!(access.community_id, "community");
    std::mem::forget(access);
    server.abort();
}
