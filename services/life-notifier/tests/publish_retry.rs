#[path = "../src/message.rs"]
mod message;
#[allow(dead_code)]
#[path = "../src/outbox_client.rs"]
mod outbox_client;

use nostr::{nips::nip59::extract_rumor, Keys};
use outbox_client::{Envelope, ResourceRef, Target};
use uuid::Uuid;

#[tokio::test]
async fn dm_retry_keeps_stable_business_dedup_identity() {
    let sender = Keys::generate();
    let receiver = Keys::generate();
    let item = Envelope {
        outbox_id: Uuid::new_v4(),
        lease_token: "l".repeat(43),
        target: Target::Dm {
            community_id: "c".into(),
            pubkey: receiver.public_key().to_hex(),
        },
        category: "project_status".into(),
        sanitized_summary: "一个项目已创建".into(),
        resource_ref: ResourceRef {
            scheme: "life".into(),
            resource_type: "project".into(),
            id: "p1".into(),
            version: 1,
        },
        idempotency_key: format!("sha256:{}", "d".repeat(64)),
        trace_id: Uuid::new_v4(),
        created_at: chrono::Utc::now(),
    };
    let idempotency = format!("sha256:{}", "d".repeat(64));
    for _ in 0..2 {
        let event = message::build_event(&item, &sender).await.expect("event");
        let rumor = extract_rumor(&receiver, &event).await.expect("rumor").rumor;
        assert!(rumor
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["idempotency", idempotency.as_str()]));
    }
}
