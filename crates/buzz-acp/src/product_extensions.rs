//! Product-specific ACP integrations composed behind generic turn contracts.

use std::sync::Arc;

use crate::turn_extension_registry::TurnExtensionRegistry;
use crate::turn_observer::TurnExtension;

#[path = "business_agent.rs"]
mod business_agent;
#[path = "business_response.rs"]
mod business_response;
#[path = "life_agent.rs"]
mod life_agent;
#[path = "life_response.rs"]
mod life_response;

pub(crate) fn load_from_env(agent_command: &str) -> Result<Arc<TurnExtensionRegistry>, String> {
    build_registry(
        business_agent::BusinessAgentHostConfig::from_env()?,
        life_agent::LifeAgentHostConfig::from_env()?,
        agent_command,
    )
}

fn build_registry(
    business: Option<business_agent::BusinessAgentHostConfig>,
    life: Option<life_agent::LifeAgentHostConfig>,
    agent_command: &str,
) -> Result<Arc<TurnExtensionRegistry>, String> {
    let extensions = business
        .map(|extension| Arc::new(extension) as Arc<dyn TurnExtension>)
        .into_iter()
        .chain(life.map(|extension| Arc::new(extension) as Arc<dyn TurnExtension>))
        .collect();
    let registry =
        Arc::new(TurnExtensionRegistry::new(extensions).map_err(|error| error.to_string())?);
    if !registry.is_empty()
        && crate::config::normalize_agent_command_identity(agent_command) != "buzz-agent"
    {
        tracing::warn!(
            runtime = agent_command,
            "Turn extension is using a general-purpose ACP runtime; ordinary MCP servers are removed, but the runtime may expose its own built-in tools"
        );
    }
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        turn_extension_registry::RegistryError,
        turn_observer::{VerifiedConversation, VerifiedTurnContext},
    };
    use nostr::{Event, EventBuilder, Keys, Kind, Tag};
    use uuid::Uuid;

    fn signed_turn(content: &str, channel_id: Uuid) -> Event {
        EventBuilder::new(Kind::Custom(40002), content)
            .tags([Tag::parse(["h", &channel_id.to_string()]).expect("channel tag")])
            .sign_with_keys(&Keys::generate())
            .expect("signed turn")
    }

    fn select_in_channel(
        registry: &TurnExtensionRegistry,
        content: &str,
        channel_type: &str,
    ) -> Result<Option<&'static str>, RegistryError> {
        let channel_id = Uuid::new_v4();
        let event = signed_turn(content, channel_id);
        registry
            .select(&VerifiedTurnContext {
                source_event: Some(&event),
                source_event_id: Some(event.id),
                source_pubkey: Some(event.pubkey),
                community_id: "community",
                conversation: VerifiedConversation::Channel {
                    channel_id,
                    channel_type: Some(channel_type.into()),
                },
                agent_id: "agent",
                agent_turn_id: "turn",
                trace_id: "trace",
            })
            .map(|selected| selected.map(|extension| extension.id()))
    }

    #[test]
    fn disabled_product_extensions_build_an_empty_registry() {
        let registry = build_registry(None, None, "buzz-agent").expect("registry");
        assert!(registry.ids().is_empty());
    }

    #[test]
    fn configured_business_extension_is_registered_once() {
        let registry = build_registry(
            Some(business_agent::BusinessAgentHostConfig::test_mock()),
            None,
            "buzz-agent",
        )
        .expect("registry");
        assert_eq!(registry.ids(), ["business"]);
    }

    #[test]
    fn configured_life_extension_is_registered_once() {
        let registry = build_registry(
            None,
            Some(life_agent::LifeAgentHostConfig::test_mock()),
            "buzz-agent",
        )
        .expect("registry");
        assert_eq!(registry.ids(), ["life"]);
    }

    #[test]
    fn life_and_business_extensions_fail_closed_at_cross_domain_boundaries() {
        let registry = build_registry(
            Some(business_agent::BusinessAgentHostConfig::test_mock()),
            Some(life_agent::LifeAgentHostConfig::test_mock()),
            "buzz-agent",
        )
        .expect("registry");

        assert_eq!(
            select_in_channel(&registry, "打开 life://action/action-1", "dm")
                .expect("Life selection"),
            Some("life")
        );
        assert!(matches!(
            select_in_channel(&registry, "今天有什么安排", "dm"),
            Err(RegistryError::Ambiguous { .. })
        ));
        assert!(matches!(
            select_in_channel(
                &registry,
                "比较 life://action/action-1 和 biz://sales-order/order-1",
                "dm",
            ),
            Err(RegistryError::Ambiguous { .. })
        ));
        assert!(matches!(
            select_in_channel(&registry, "打开 life://action/action-1", "stream"),
            Err(RegistryError::Ambiguous { .. })
        ));
    }
}
