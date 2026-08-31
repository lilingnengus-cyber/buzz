//! Product-specific ACP integrations composed behind generic turn contracts.

use std::sync::Arc;

use crate::turn_extension_registry::TurnExtensionRegistry;
use crate::turn_observer::TurnExtension;

#[path = "business_agent.rs"]
mod business_agent;
#[path = "business_response.rs"]
mod business_response;

pub(crate) fn load_from_env(agent_command: &str) -> Result<Arc<TurnExtensionRegistry>, String> {
    build_registry(
        business_agent::BusinessAgentHostConfig::from_env()?,
        agent_command,
    )
}

fn build_registry(
    business: Option<business_agent::BusinessAgentHostConfig>,
    agent_command: &str,
) -> Result<Arc<TurnExtensionRegistry>, String> {
    let extensions = business
        .map(|extension| Arc::new(extension) as Arc<dyn TurnExtension>)
        .into_iter()
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

    #[test]
    fn disabled_product_extensions_build_an_empty_registry() {
        let registry = build_registry(None, "buzz-agent").expect("registry");
        assert!(registry.ids().is_empty());
    }

    #[test]
    fn configured_business_extension_is_registered_once() {
        let registry = build_registry(
            Some(business_agent::BusinessAgentHostConfig::test_mock()),
            "buzz-agent",
        )
        .expect("registry");
        assert_eq!(registry.ids(), ["business"]);
    }
}
