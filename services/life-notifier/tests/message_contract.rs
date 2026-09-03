#[path = "../src/message.rs"]
mod message;
#[allow(dead_code)]
#[path = "../src/outbox_client.rs"]
mod outbox_client;

use nostr::{nips::nip59::extract_rumor, Keys, Kind};
use outbox_client::{Envelope, ResourceRef, Target};
use uuid::Uuid;

fn envelope(target: Target) -> Envelope {
    Envelope {
        outbox_id: Uuid::new_v4(),
        lease_token: "l".repeat(43),
        target,
        category: "action_summary".into(),
        sanitized_summary: "一个行动状态已更新".into(),
        resource_ref: ResourceRef {
            scheme: "life".into(),
            resource_type: "action".into(),
            id: "action-1".into(),
            version: 2,
        },
        idempotency_key: format!("sha256:{}", "d".repeat(64)),
        trace_id: Uuid::new_v4(),
        created_at: chrono::Utc::now(),
    }
}

#[tokio::test]
async fn dm_is_encrypted_and_contract_tags_are_inside_rumor() {
    let sender = Keys::generate();
    let receiver = Keys::generate();
    let event = message::build_event(
        &envelope(Target::Dm {
            community_id: "c".into(),
            pubkey: receiver.public_key().to_hex(),
        }),
        &sender,
    )
    .await
    .expect("DM");
    assert_eq!(event.kind, Kind::GiftWrap);
    assert!(!event.content.contains("行动"));
    let unwrapped = extract_rumor(&receiver, &event).await.expect("unwrap");
    assert!(unwrapped
        .rumor
        .tags
        .iter()
        .any(|tag| tag.as_slice() == ["source", "life-notifier"]));
    let idempotency = format!("sha256:{}", "d".repeat(64));
    assert!(unwrapped
        .rumor
        .tags
        .iter()
        .any(|tag| tag.as_slice() == ["idempotency", idempotency.as_str()]));
}

#[tokio::test]
async fn malformed_business_idempotency_is_rejected() {
    let receiver = Keys::generate();
    let mut item = envelope(Target::Dm {
        community_id: "c".into(),
        pubkey: receiver.public_key().to_hex(),
    });
    item.idempotency_key = "sha256:not-a-digest".into();
    assert!(message::build_event(&item, &Keys::generate())
        .await
        .is_err());
}

#[tokio::test]
async fn channel_uses_existing_message_kind_and_h_scope() {
    let event = message::build_event(
        &envelope(Target::Channel {
            community_id: "c".into(),
            channel_id: "channel-1".into(),
        }),
        &Keys::generate(),
    )
    .await
    .expect("channel");
    assert_eq!(event.kind.as_u16(), 40002);
    assert!(event
        .tags
        .iter()
        .any(|tag| tag.as_slice() == ["h", "channel-1"]));
}
