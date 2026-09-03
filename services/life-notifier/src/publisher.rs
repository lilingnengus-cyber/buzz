use crate::{message, outbox_client::Envelope};
use buzz_ws_client::publish_event;
use nostr::Keys;

pub(crate) async fn publish(
    relay_url: &str,
    keys: &Keys,
    item: &Envelope,
) -> anyhow::Result<String> {
    let event = message::build_event(item, keys).await?;
    let event_id = event.id.to_hex();
    let result = publish_event(relay_url, event, keys, None, 40).await?;
    if !result.accepted {
        anyhow::bail!("relay rejected notification event");
    }
    Ok(event_id)
}
