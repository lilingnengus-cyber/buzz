use anyhow::{ensure, Context, Result};
use buzz_db::event::EventQuery;
use nostr::{Event, EventBuilder, Keys, Kind, Tag, Timestamp};

fn repaired_event(old: &Event, keys: &Keys) -> Result<Event> {
    ensure!(
        old.pubkey == keys.public_key(),
        "metadata signer does not match configured relay key"
    );
    ensure!(old.kind == Kind::Custom(39000), "expected channel metadata");
    old.verify()?;
    let mut tags: Vec<Tag> = old
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().is_none_or(|name| name != "archived"))
        .cloned()
        .collect();
    tags.push(Tag::parse(["archived", "true"])?);
    Ok(EventBuilder::new(old.kind, old.content.clone())
        .tags(tags)
        .custom_created_at(Timestamp::from(
            Timestamp::now()
                .as_secs()
                .max(old.created_at.as_secs().saturating_add(1)),
        ))
        .sign_with_keys(keys)?)
}

pub(super) async fn run(channel: uuid::Uuid, apply: bool) -> Result<()> {
    let db = super::connect_db().await?;
    let tenant = super::resolve_admin_tenant(&db).await?;
    let row = db.get_channel(tenant.community(), channel).await?;
    ensure!(
        row.archived_at.is_some(),
        "channel is not archived; refusing repair"
    );
    let keys = Keys::parse(
        &std::env::var("BUZZ_RELAY_PRIVATE_KEY").context("relay key must be configured")?,
    )?;
    let records = db
        .query_events(&EventQuery {
            kinds: Some(vec![39000]),
            d_tag: Some(channel.to_string()),
            limit: Some(1),
            ..EventQuery::for_community(tenant.community())
        })
        .await?;
    let old = &records
        .first()
        .context("canonical channel metadata is missing")?
        .event;
    let event = repaired_event(old, &keys)?;
    let already = old
        .tags
        .iter()
        .any(|tag| tag.as_slice() == ["archived", "true"]);
    println!(
        "channel={channel} old_event={} already_archived={already} apply={apply}",
        old.id
    );
    if !apply || already {
        return Ok(());
    }
    ensure!(
        db.get_channel(tenant.community(), channel)
            .await?
            .archived_at
            .is_some(),
        "channel was restored; refusing repair"
    );
    db.replace_addressable_event(tenant.community(), &event, Some(channel))
        .await?;
    println!(
        "Repaired metadata event={}; clients receive it on channel refresh",
        event.id
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_metadata_and_rejects_wrong_signer() -> Result<()> {
        let keys = Keys::generate();
        let old = EventBuilder::new(Kind::Custom(39000), "retained content")
            .tags([
                Tag::parse(["d", "channel-id"])?,
                Tag::parse(["ttl", "3600"])?,
                Tag::parse(["private"])?,
                Tag::parse(["custom", "retained"])?,
            ])
            .sign_with_keys(&keys)?;
        let new = repaired_event(&old, &keys)?;
        new.verify()?;
        assert_eq!(new.content, old.content);
        assert!(new.created_at > old.created_at);
        for tag in old.tags.iter() {
            assert!(new.tags.iter().any(|item| item == tag));
        }
        assert!(new
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["archived", "true"]));
        assert!(repaired_event(&old, &Keys::generate()).is_err());
        Ok(())
    }
}
