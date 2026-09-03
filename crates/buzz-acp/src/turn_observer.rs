//! Optional per-turn observation hook for product-specific ACP integrations.
//!
//! The ACP client owns protocol transport. Integrations may observe normalized
//! `session/update` payloads, but must keep product policy and result parsing
//! outside the transport implementation.

use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use nostr::{Event, EventId, PublicKey};
use uuid::Uuid;

use crate::acp::{AcpClient, McpServer};
use crate::relay::RestClient;

pub(crate) trait TurnObserver: Send {
    fn on_session_update(&mut self, update: &serde_json::Value);

    fn into_any(self: Box<Self>) -> Box<dyn Any + Send>;
}

pub(crate) type TurnExtensionFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnMcpMode {
    AppendStandard,
    ReplaceStandard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TurnPolicy {
    pub(crate) mcp_mode: TurnMcpMode,
    pub(crate) max_turn_duration: Option<Duration>,
    pub(crate) base_prompt: Option<&'static str>,
    pub(crate) disable_memory: bool,
    pub(crate) requires_fresh_session: bool,
}

impl Default for TurnPolicy {
    fn default() -> Self {
        Self {
            mcp_mode: TurnMcpMode::AppendStandard,
            max_turn_duration: None,
            base_prompt: None,
            disable_memory: false,
            requires_fresh_session: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VerifiedConversation {
    Heartbeat,
    Channel {
        channel_id: Uuid,
        channel_type: Option<String>,
        /// Exact current member pubkeys from the relay's kind-39002 event.
        participant_pubkeys: Vec<String>,
    },
}

/// Host-owned facts for one turn. Product extensions must not reconstruct
/// identity or routing facts from prompt text.
pub(crate) struct VerifiedTurnContext<'a> {
    pub(crate) source_event: Option<&'a Event>,
    pub(crate) source_event_id: Option<EventId>,
    pub(crate) source_pubkey: Option<PublicKey>,
    pub(crate) community_id: &'a str,
    pub(crate) conversation: VerifiedConversation,
    pub(crate) agent_id: &'a str,
    pub(crate) agent_turn_id: &'a str,
    pub(crate) trace_id: &'a str,
}

impl VerifiedTurnContext<'_> {
    pub(crate) fn channel_id(&self) -> Option<Uuid> {
        match self.conversation {
            VerifiedConversation::Channel { channel_id, .. } => Some(channel_id),
            VerifiedConversation::Heartbeat => None,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.community_id.trim().is_empty()
            || self.agent_id.trim().is_empty()
            || self.agent_turn_id.trim().is_empty()
            || self.trace_id.trim().is_empty()
        {
            return Err("verified turn identity fields must not be empty".into());
        }
        match self.source_event {
            Some(event)
                if self.source_event_id == Some(event.id)
                    && self.source_pubkey == Some(event.pubkey)
                    && self.channel_id().is_some() =>
            {
                Ok(())
            }
            None if self.source_event_id.is_none()
                && self.source_pubkey.is_none()
                && self.channel_id().is_none() =>
            {
                Ok(())
            }
            _ => Err("verified turn event and conversation facts are inconsistent".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum TurnApplicability {
    #[default]
    NotApplicable,
    Applicable {
        priority: u16,
        reason: &'static str,
    },
    Ambiguous {
        reason: &'static str,
    },
}

pub(crate) struct TurnExtensionFinishContext<'a> {
    pub(crate) acp: &'a mut AcpClient,
    pub(crate) rest_client: &'a RestClient,
    pub(crate) source_event: Option<&'a Event>,
    pub(crate) channel_id: Option<Uuid>,
    pub(crate) completed: bool,
}

/// Product integration applied to one ACP turn.
///
/// Implementations may inject an ephemeral MCP server and observe the turn,
/// but the pool remains unaware of product policy and credentials.
pub(crate) trait TurnExtension: Send + Sync {
    fn id(&self) -> &'static str;

    fn classify_turn(&self, context: &VerifiedTurnContext<'_>)
        -> Result<TurnApplicability, String>;

    fn begin_turn<'a>(
        &'a self,
        context: VerifiedTurnContext<'a>,
    ) -> TurnExtensionFuture<'a, Result<Option<Box<dyn TurnExtensionAccess>>, String>>;
}

/// Per-turn state returned by a [`TurnExtension`]. Dropping it must release
/// any capability that should not survive the turn.
pub(crate) trait TurnExtensionAccess: Send {
    fn policy(&self) -> &TurnPolicy;

    fn mcp_server(&self) -> Option<&McpServer>;

    fn start_observation(&mut self, acp: &mut AcpClient);

    fn finish<'a>(
        &'a mut self,
        context: TurnExtensionFinishContext<'a>,
    ) -> TurnExtensionFuture<'a, ()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys};

    #[test]
    fn verified_turn_context_uses_signed_event_identity() {
        let keys = Keys::generate();
        let event = EventBuilder::text_note("prompt text cannot replace these fields")
            .sign_with_keys(&keys)
            .expect("signed event");
        let channel_id = Uuid::new_v4();
        let context = VerifiedTurnContext {
            source_event: Some(&event),
            source_event_id: Some(event.id),
            source_pubkey: Some(event.pubkey),
            community_id: "https://community.example",
            conversation: VerifiedConversation::Channel {
                channel_id,
                channel_type: Some("stream".into()),
                participant_pubkeys: vec![event.pubkey.to_hex()],
            },
            agent_id: "agent",
            agent_turn_id: "turn",
            trace_id: "trace",
        };

        assert_eq!(context.source_event_id, Some(event.id));
        assert_eq!(context.source_pubkey, Some(event.pubkey));
        assert_eq!(context.channel_id(), Some(channel_id));
        assert_eq!(context.community_id, "https://community.example");
        assert!(context.validate().is_ok());
    }

    #[test]
    fn default_turn_policy_preserves_standard_runtime_behavior() {
        assert_eq!(
            TurnPolicy::default(),
            TurnPolicy {
                mcp_mode: TurnMcpMode::AppendStandard,
                max_turn_duration: None,
                base_prompt: None,
                disable_memory: false,
                requires_fresh_session: false,
            }
        );
    }
}
