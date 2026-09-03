use crate::outbox_client::{Envelope, Target};
use nostr::{Event, EventBuilder, Keys, Kind, PublicKey, Tag};

const STREAM_MESSAGE_KIND: u16 = 40002;

pub(crate) async fn build_event(item: &Envelope, keys: &Keys) -> anyhow::Result<Event> {
    validate(item)?;
    let content = format!(
        "{}\n\nlife://{}/{}?version={}",
        item.sanitized_summary,
        item.resource_ref.resource_type,
        item.resource_ref.id,
        item.resource_ref.version
    );
    let tags = contract_tags(item)?;
    match &item.target {
        Target::Dm { pubkey, .. } => {
            let receiver = PublicKey::parse(pubkey)?;
            Ok(EventBuilder::private_msg(keys, receiver, content, tags).await?)
        }
        Target::Channel { channel_id, .. } => Ok(EventBuilder::new(
            Kind::Custom(STREAM_MESSAGE_KIND),
            content,
        )
        .tags(std::iter::once(Tag::parse(["h", channel_id])?).chain(tags))
        .sign_with_keys(keys)?),
    }
}

fn contract_tags(item: &Envelope) -> anyhow::Result<Vec<Tag>> {
    Ok(vec![
        Tag::parse(["source", "life-notifier"])?,
        Tag::parse(["idempotency", item.idempotency_key.as_str()])?,
        Tag::parse(["trace", &item.trace_id.to_string()])?,
        Tag::parse(["category", item.category.as_str()])?,
    ])
}

pub(crate) fn validate(item: &Envelope) -> anyhow::Result<()> {
    if item.resource_ref.scheme != "life"
        || !valid_summary(&item.category, &item.sanitized_summary)
        || !valid_idempotency_key(&item.idempotency_key)
        || !safe_opaque(&item.resource_ref.resource_type, 64)
        || !safe_opaque(&item.resource_ref.id, 256)
        || item.resource_ref.version < 1
        || item.lease_token.len() < 32
        || item.lease_token.len() > 256
        || item.lease_token.chars().any(char::is_whitespace)
    {
        anyhow::bail!("outbox envelope violates the notification contract");
    }
    match &item.target {
        Target::Dm {
            community_id,
            pubkey,
        } if !safe_opaque(community_id, 256)
            || pubkey.len() != 64
            || !pubkey
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) =>
        {
            anyhow::bail!("invalid DM target")
        }
        Target::Channel {
            community_id,
            channel_id,
        } if !safe_opaque(community_id, 256) || !safe_opaque(channel_id, 256) => {
            anyhow::bail!("invalid channel target")
        }
        _ => Ok(()),
    }
}

fn valid_summary(category: &str, summary: &str) -> bool {
    match category {
        "action_summary" => matches!(
            summary,
            "一个行动状态已更新" | "今日行动安排已更新" | "一个 AI 执行状态已更新"
        ),
        "project_status" => summary == "一个项目已创建",
        _ => false,
    }
}

fn safe_opaque(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._~:-".contains(&byte))
}

fn valid_idempotency_key(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}
