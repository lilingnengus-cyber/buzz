//! Prevents service-originated LifeOS notifications from recursively starting Agent turns.

use nostr::Event;

pub(crate) fn is_life_notifier_event(event: &Event) -> bool {
    event
        .tags
        .iter()
        .any(|tag| tag.as_slice() == ["source", "life-notifier"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    #[test]
    fn only_exact_source_tag_is_guarded() {
        let event = EventBuilder::new(Kind::Custom(40002), "life://action/a")
            .tags([Tag::parse(["source", "life-notifier"]).expect("tag")])
            .sign_with_keys(&Keys::generate())
            .expect("event");
        assert!(is_life_notifier_event(&event));

        let user_reply = EventBuilder::new(Kind::Custom(40002), "请查看这条通知")
            .sign_with_keys(&Keys::generate())
            .expect("reply");
        assert!(!is_life_notifier_event(&user_reply));
    }
}
