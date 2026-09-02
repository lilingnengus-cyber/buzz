//! LifeOS authorization policy independent from Pacioli transport and storage.
//!
//! The gateway resolves durable identities, memberships, catalog entries, and
//! runtime ceilings before calling [`evaluate`]. This crate only computes a
//! deterministic least-privilege intersection and performs no I/O.

#![deny(missing_docs)]

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A stable LifeOS capability identifier such as `action:update`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Capability(String);

impl Capability {
    /// Parses a portable, lower-case capability identifier.
    pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        let valid = (3..=128).contains(&value.len())
            && value.contains(':')
            && !value.starts_with(':')
            && !value.ends_with(':')
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_:.-".contains(&byte)
            });
        valid.then_some(Self(value)).ok_or("invalid_capability")
    }

    /// Returns the stable string identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One independently intersected dimension of a LifeOS data scope.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "values", rename_all = "snake_case")]
pub enum ScopeSet {
    /// Adds no restriction for this dimension inside the current Life domain.
    #[default]
    Unrestricted,
    /// Restricts this dimension to a non-empty set of opaque identifiers.
    Restricted(BTreeSet<String>),
}

impl ScopeSet {
    /// Constructs a restricted set after rejecting empty or malformed values.
    pub fn restricted<I, S>(values: I) -> Result<Self, &'static str>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let values = values.into_iter().map(Into::into).collect::<BTreeSet<_>>();
        if values.is_empty()
            || values.iter().any(|value| {
                value.is_empty()
                    || value.len() > 256
                    || value.trim() != value
                    || value.chars().any(char::is_control)
            })
        {
            return Err("invalid_scope_set");
        }
        Ok(Self::Restricted(values))
    }

    /// Computes the least-privilege intersection for one scope dimension.
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        match (self, other) {
            (Self::Unrestricted, scope) | (scope, Self::Unrestricted) => Some(scope.clone()),
            (Self::Restricted(left), Self::Restricted(right)) => {
                let values = left.intersection(right).cloned().collect::<BTreeSet<_>>();
                (!values.is_empty()).then_some(Self::Restricted(values))
            }
        }
    }
}

/// Deterministic LifeOS data restrictions applied to a capability.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataScope {
    /// Allowed LifeOS workspace identifiers.
    pub workspaces: ScopeSet,
    /// Allowed LifeOS domain identifiers.
    pub domains: ScopeSet,
    /// Allowed LifeOS project identifiers.
    pub projects: ScopeSet,
    /// Allowed opaque resource identifiers.
    pub resources: ScopeSet,
    /// Allowed sensitivity classifications.
    pub sensitivities: ScopeSet,
    /// Allowed operation-count buckets.
    pub operation_count: ScopeSet,
}

impl DataScope {
    /// Intersects all six dimensions, rejecting the scope if any is disjoint.
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        Some(Self {
            workspaces: self.workspaces.intersection(&other.workspaces)?,
            domains: self.domains.intersection(&other.domains)?,
            projects: self.projects.intersection(&other.projects)?,
            resources: self.resources.intersection(&other.resources)?,
            sensitivities: self.sensitivities.intersection(&other.sensitivities)?,
            operation_count: self.operation_count.intersection(&other.operation_count)?,
        })
    }
}

/// A restriction that must accompany an otherwise permitted capability.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Obligation {
    /// Requires a human to confirm the proposed operation.
    HumanConfirmation,
    /// Requires a fresh higher-assurance authentication step.
    StepUpAuthentication,
    /// Requires approval from a second authorized person.
    DualControl,
    /// Restricts the capability to a direct-message turn.
    DmOnly,
    /// Requires sensitive fields to be redacted from returned content.
    RedactSensitive,
    /// Caps one operation to the given positive batch size.
    MaxBatch(u32),
}

/// One persistent or runtime capability ceiling.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityGrant {
    /// Data restrictions attached to the capability.
    pub data_scope: DataScope,
    /// Mandatory restrictions attached to the capability.
    pub obligations: BTreeSet<Obligation>,
}

/// A caller-requested capability and optional additional restrictions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityRequest {
    /// Requested data scope, intersected with authority and runtime ceilings.
    pub data_scope: DataScope,
    /// Additional obligations requested by the trusted host policy.
    pub obligations: BTreeSet<Obligation>,
}

/// The currently resolved, durable authority for one subject.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Authority {
    /// Opaque stable identifier used for audit correlation.
    pub principal_id: String,
    /// Whether this principal may participate in new decisions.
    pub active: bool,
    /// Current capability grants, after role and direct-grant resolution.
    pub grants: BTreeMap<Capability, CapabilityGrant>,
}

/// The mutually exclusive authority source for a Life Agent turn.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "authority", rename_all = "snake_case")]
pub enum SubjectAuthority {
    /// A proxy Agent uses only the initiating human's current authority.
    Human(Authority),
    /// An independent Agent uses only its own persistent authority.
    IndependentAgent(Authority),
}

impl SubjectAuthority {
    fn authority(&self) -> &Authority {
        match self {
            Self::Human(authority) | Self::IndependentAgent(authority) => authority,
        }
    }
}

/// Trusted conversation classification relevant to policy obligations.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationContext {
    /// A verified one-to-one direct-message turn.
    DirectMessage,
    /// A verified channel or group conversation with multiple participants.
    MultiPartyChannel,
}

/// Fully resolved input for one deterministic authorization decision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationInput {
    /// Exactly one authority source for the current Agent.
    pub subject: SubjectAuthority,
    /// Capabilities and scopes requested for this turn.
    pub requested: BTreeMap<Capability, CapabilityRequest>,
    /// Trusted host-policy ceiling; omitted capabilities are denied.
    pub runtime_ceiling: BTreeMap<Capability, CapabilityGrant>,
    /// Verified DM or multi-party channel classification.
    pub conversation: ConversationContext,
}

impl EvaluationInput {
    /// Constructs a proxy-Agent decision from the human's current authority.
    pub fn proxy_agent(
        human: Authority,
        requested: BTreeMap<Capability, CapabilityRequest>,
        runtime_ceiling: BTreeMap<Capability, CapabilityGrant>,
        conversation: ConversationContext,
    ) -> Self {
        Self {
            subject: SubjectAuthority::Human(human),
            requested,
            runtime_ceiling,
            conversation,
        }
    }

    /// Constructs an independent-Agent decision from only its own authority.
    pub fn independent_agent(
        agent: Authority,
        requested: BTreeMap<Capability, CapabilityRequest>,
        runtime_ceiling: BTreeMap<Capability, CapabilityGrant>,
        conversation: ConversationContext,
    ) -> Self {
        Self {
            subject: SubjectAuthority::IndependentAgent(agent),
            requested,
            runtime_ceiling,
            conversation,
        }
    }
}

/// Effective capability, scope, and obligations from one positive intersection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveGrant {
    /// Final six-dimensional data scope.
    pub data_scope: DataScope,
    /// Union of restrictions with the strictest `MaxBatch` limit.
    pub obligations: BTreeSet<Obligation>,
}

/// Stable high-level classification for an authorization decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionReason {
    /// Every requested capability was allowed.
    Allowed,
    /// At least one requested capability was allowed and one denied.
    PartiallyAllowed,
    /// No requested capability survived the intersection.
    NoEffectivePermission,
    /// The selected human or independent-Agent authority is inactive.
    SubjectInactive,
    /// The request did not contain any capabilities.
    EmptyRequest,
}

/// Deterministic, auditable outcome for one Life Agent turn.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    /// Whether at least one requested capability is effective.
    pub allowed: bool,
    /// Stable overall outcome classification.
    pub reason: DecisionReason,
    /// Capabilities that survived every intersection and obligation check.
    pub allowed_capabilities: BTreeSet<Capability>,
    /// Capabilities denied by any authority, scope, ceiling, or obligation gate.
    pub denied_capabilities: BTreeSet<Capability>,
    /// Effective grants keyed by allowed capability.
    pub grants: BTreeMap<Capability, EffectiveGrant>,
}

/// Evaluates one Life Agent authorization request without performing I/O.
///
/// Capabilities missing from either durable authority or the trusted runtime
/// ceiling are denied. Data scope is intersected dimension-by-dimension, while
/// obligations accumulate and can never be removed by a narrower layer.
pub fn evaluate(input: EvaluationInput) -> Decision {
    let requested_capabilities = input.requested.keys().cloned().collect::<BTreeSet<_>>();
    if requested_capabilities.is_empty() {
        return denied_decision(DecisionReason::EmptyRequest, requested_capabilities);
    }

    let authority = input.subject.authority();
    if !authority.active {
        return denied_decision(DecisionReason::SubjectInactive, requested_capabilities);
    }

    let mut grants = BTreeMap::new();
    let mut denied_capabilities = BTreeSet::new();
    for (capability, request) in input.requested {
        let Some(authority_grant) = authority.grants.get(&capability) else {
            denied_capabilities.insert(capability);
            continue;
        };
        let Some(runtime_grant) = input.runtime_ceiling.get(&capability) else {
            denied_capabilities.insert(capability);
            continue;
        };
        let Some(data_scope) = authority_grant
            .data_scope
            .intersection(&request.data_scope)
            .and_then(|scope| scope.intersection(&runtime_grant.data_scope))
        else {
            denied_capabilities.insert(capability);
            continue;
        };
        let Some(obligations) = merge_obligations([
            &authority_grant.obligations,
            &request.obligations,
            &runtime_grant.obligations,
        ]) else {
            denied_capabilities.insert(capability);
            continue;
        };
        if obligations.contains(&Obligation::DmOnly)
            && input.conversation != ConversationContext::DirectMessage
        {
            denied_capabilities.insert(capability);
            continue;
        }
        grants.insert(
            capability,
            EffectiveGrant {
                data_scope,
                obligations,
            },
        );
    }

    let allowed_capabilities = grants.keys().cloned().collect::<BTreeSet<_>>();
    let allowed = !allowed_capabilities.is_empty();
    let reason = match (allowed, denied_capabilities.is_empty()) {
        (true, true) => DecisionReason::Allowed,
        (true, false) => DecisionReason::PartiallyAllowed,
        (false, _) => DecisionReason::NoEffectivePermission,
    };
    Decision {
        allowed,
        reason,
        allowed_capabilities,
        denied_capabilities,
        grants,
    }
}

fn merge_obligations<'a>(
    sources: impl IntoIterator<Item = &'a BTreeSet<Obligation>>,
) -> Option<BTreeSet<Obligation>> {
    let mut obligations = BTreeSet::new();
    let mut max_batch = None;
    for obligation in sources.into_iter().flatten() {
        match obligation {
            Obligation::MaxBatch(0) => return None,
            Obligation::MaxBatch(limit) => {
                max_batch = Some(max_batch.map_or(*limit, |current: u32| current.min(*limit)));
            }
            obligation => {
                obligations.insert(obligation.clone());
            }
        }
    }
    if let Some(limit) = max_batch {
        obligations.insert(Obligation::MaxBatch(limit));
    }
    Some(obligations)
}

fn denied_decision(reason: DecisionReason, denied_capabilities: BTreeSet<Capability>) -> Decision {
    Decision {
        allowed: false,
        reason,
        allowed_capabilities: BTreeSet::new(),
        denied_capabilities,
        grants: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn capability(value: &str) -> Capability {
        Capability::parse(value).expect("valid test capability")
    }

    fn restricted(values: &[&str]) -> ScopeSet {
        ScopeSet::restricted(values.iter().copied()).expect("non-empty test scope")
    }

    fn scope(
        workspaces: &[&str],
        domains: &[&str],
        projects: &[&str],
        resources: &[&str],
        sensitivities: &[&str],
        operation_count: &[&str],
    ) -> DataScope {
        DataScope {
            workspaces: restricted(workspaces),
            domains: restricted(domains),
            projects: restricted(projects),
            resources: restricted(resources),
            sensitivities: restricted(sensitivities),
            operation_count: restricted(operation_count),
        }
    }

    fn unrestricted_grant() -> CapabilityGrant {
        CapabilityGrant {
            data_scope: DataScope::default(),
            obligations: BTreeSet::new(),
        }
    }

    fn authority(entries: &[(&str, CapabilityGrant)]) -> Authority {
        Authority {
            principal_id: "principal-1".into(),
            active: true,
            grants: entries
                .iter()
                .map(|(name, grant)| (capability(name), grant.clone()))
                .collect(),
        }
    }

    fn requested(names: &[&str]) -> BTreeMap<Capability, CapabilityRequest> {
        names
            .iter()
            .map(|name| {
                (
                    capability(name),
                    CapabilityRequest {
                        data_scope: DataScope::default(),
                        obligations: BTreeSet::new(),
                    },
                )
            })
            .collect()
    }

    fn ceiling(names: &[&str]) -> BTreeMap<Capability, CapabilityGrant> {
        names
            .iter()
            .map(|name| (capability(name), unrestricted_grant()))
            .collect()
    }

    #[test]
    fn independent_agent_never_inherits_human_write_authority() {
        let decision = evaluate(EvaluationInput::independent_agent(
            authority(&[("action:read", unrestricted_grant())]),
            requested(&["action:read", "action:update"]),
            ceiling(&["action:read", "action:update"]),
            ConversationContext::DirectMessage,
        ));

        assert_eq!(
            decision.allowed_capabilities,
            BTreeSet::from([capability("action:read")])
        );
        assert_eq!(
            decision.denied_capabilities,
            BTreeSet::from([capability("action:update")])
        );
        assert_eq!(decision.reason, DecisionReason::PartiallyAllowed);
    }

    #[test]
    fn proxy_agent_uses_current_human_authority() {
        let decision = evaluate(EvaluationInput::proxy_agent(
            authority(&[("journal:create", unrestricted_grant())]),
            requested(&["journal:create", "journal:delete"]),
            ceiling(&["journal:create", "journal:delete"]),
            ConversationContext::DirectMessage,
        ));

        assert!(decision
            .allowed_capabilities
            .contains(&capability("journal:create")));
        assert!(decision
            .denied_capabilities
            .contains(&capability("journal:delete")));
    }

    #[test]
    fn every_data_scope_dimension_is_intersected() {
        let authority_scope = scope(
            &["home", "shared"],
            &["health", "work"],
            &["p1", "p2"],
            &["a1", "a2"],
            &["normal", "private"],
            &["1", "5"],
        );
        let requested_scope = scope(&["home"], &["work"], &["p2"], &["a2"], &["private"], &["1"]);
        let runtime_scope = scope(
            &["home"],
            &["work", "learning"],
            &["p2"],
            &["a2", "a3"],
            &["private"],
            &["1", "2"],
        );
        let name = capability("action:update");
        let decision = evaluate(EvaluationInput::proxy_agent(
            authority(&[(
                "action:update",
                CapabilityGrant {
                    data_scope: authority_scope,
                    obligations: BTreeSet::new(),
                },
            )]),
            BTreeMap::from([(
                name.clone(),
                CapabilityRequest {
                    data_scope: requested_scope.clone(),
                    obligations: BTreeSet::new(),
                },
            )]),
            BTreeMap::from([(
                name.clone(),
                CapabilityGrant {
                    data_scope: runtime_scope,
                    obligations: BTreeSet::new(),
                },
            )]),
            ConversationContext::DirectMessage,
        ));

        assert_eq!(decision.grants[&name].data_scope, requested_scope);
    }

    #[test]
    fn a_disjoint_scope_dimension_denies_the_capability() {
        let name = capability("project:read");
        let decision = evaluate(EvaluationInput::proxy_agent(
            authority(&[(
                "project:read",
                CapabilityGrant {
                    data_scope: DataScope {
                        workspaces: restricted(&["home"]),
                        ..DataScope::default()
                    },
                    obligations: BTreeSet::new(),
                },
            )]),
            BTreeMap::from([(
                name.clone(),
                CapabilityRequest {
                    data_scope: DataScope {
                        workspaces: restricted(&["other"]),
                        ..DataScope::default()
                    },
                    obligations: BTreeSet::new(),
                },
            )]),
            ceiling(&["project:read"]),
            ConversationContext::DirectMessage,
        ));

        assert!(decision.allowed_capabilities.is_empty());
        assert_eq!(decision.denied_capabilities, BTreeSet::from([name]));
    }

    #[test]
    fn obligations_only_accumulate_and_max_batch_uses_the_strictest_limit() {
        let name = capability("knowledge:export");
        let decision = evaluate(EvaluationInput::proxy_agent(
            authority(&[(
                "knowledge:export",
                CapabilityGrant {
                    data_scope: DataScope::default(),
                    obligations: BTreeSet::from([
                        Obligation::HumanConfirmation,
                        Obligation::MaxBatch(20),
                    ]),
                },
            )]),
            BTreeMap::from([(
                name.clone(),
                CapabilityRequest {
                    data_scope: DataScope::default(),
                    obligations: BTreeSet::from([Obligation::RedactSensitive]),
                },
            )]),
            BTreeMap::from([(
                name.clone(),
                CapabilityGrant {
                    data_scope: DataScope::default(),
                    obligations: BTreeSet::from([
                        Obligation::StepUpAuthentication,
                        Obligation::MaxBatch(5),
                    ]),
                },
            )]),
            ConversationContext::DirectMessage,
        ));

        assert_eq!(
            decision.grants[&name].obligations,
            BTreeSet::from([
                Obligation::HumanConfirmation,
                Obligation::StepUpAuthentication,
                Obligation::RedactSensitive,
                Obligation::MaxBatch(5),
            ])
        );
    }

    #[test]
    fn dm_only_capability_is_rejected_in_a_multi_party_channel() {
        let decision = evaluate(EvaluationInput::proxy_agent(
            authority(&[(
                "journal:read",
                CapabilityGrant {
                    data_scope: DataScope::default(),
                    obligations: BTreeSet::from([Obligation::DmOnly]),
                },
            )]),
            requested(&["journal:read"]),
            ceiling(&["journal:read"]),
            ConversationContext::MultiPartyChannel,
        ));

        assert!(!decision.allowed);
        assert_eq!(decision.reason, DecisionReason::NoEffectivePermission);
        assert!(decision
            .denied_capabilities
            .contains(&capability("journal:read")));
    }

    #[test]
    fn inactive_subject_is_fail_closed() {
        let mut human = authority(&[("goal:read", unrestricted_grant())]);
        human.active = false;
        let decision = evaluate(EvaluationInput::proxy_agent(
            human,
            requested(&["goal:read"]),
            ceiling(&["goal:read"]),
            ConversationContext::DirectMessage,
        ));

        assert!(!decision.allowed);
        assert_eq!(decision.reason, DecisionReason::SubjectInactive);
        assert_eq!(
            decision.denied_capabilities,
            BTreeSet::from([capability("goal:read")])
        );
    }

    #[test]
    fn missing_runtime_ceiling_is_fail_closed() {
        let decision = evaluate(EvaluationInput::proxy_agent(
            authority(&[("focus:read", unrestricted_grant())]),
            requested(&["focus:read"]),
            BTreeMap::new(),
            ConversationContext::DirectMessage,
        ));

        assert!(!decision.allowed);
        assert_eq!(decision.reason, DecisionReason::NoEffectivePermission);
        assert!(decision
            .denied_capabilities
            .contains(&capability("focus:read")));
    }

    #[test]
    fn empty_request_is_rejected_explicitly() {
        let decision = evaluate(EvaluationInput::proxy_agent(
            authority(&[("workspace:read", unrestricted_grant())]),
            BTreeMap::new(),
            ceiling(&["workspace:read"]),
            ConversationContext::DirectMessage,
        ));

        assert!(!decision.allowed);
        assert_eq!(decision.reason, DecisionReason::EmptyRequest);
        assert!(decision.denied_capabilities.is_empty());
    }

    #[test]
    fn malformed_capability_and_scope_values_are_rejected() {
        for invalid in ["Action:read", "read", ":read", "read:", "action read"] {
            assert_eq!(Capability::parse(invalid), Err("invalid_capability"));
        }
        assert_eq!(
            ScopeSet::restricted(Vec::<String>::new()),
            Err("invalid_scope_set")
        );
        assert_eq!(
            ScopeSet::restricted([" workspace"]),
            Err("invalid_scope_set")
        );
        assert_eq!(
            ScopeSet::restricted(["workspace\nother"]),
            Err("invalid_scope_set")
        );
    }

    #[test]
    fn zero_max_batch_is_fail_closed() {
        let decision = evaluate(EvaluationInput::proxy_agent(
            authority(&[(
                "action:create",
                CapabilityGrant {
                    data_scope: DataScope::default(),
                    obligations: BTreeSet::from([Obligation::MaxBatch(0)]),
                },
            )]),
            requested(&["action:create"]),
            ceiling(&["action:create"]),
            ConversationContext::DirectMessage,
        ));

        assert!(!decision.allowed);
        assert!(decision.grants.is_empty());
        assert!(decision
            .denied_capabilities
            .contains(&capability("action:create")));
    }

    #[test]
    fn dm_only_capability_is_allowed_in_a_direct_message() {
        let name = capability("journal:read");
        let decision = evaluate(EvaluationInput::proxy_agent(
            authority(&[(
                "journal:read",
                CapabilityGrant {
                    data_scope: DataScope::default(),
                    obligations: BTreeSet::from([Obligation::DmOnly]),
                },
            )]),
            requested(&["journal:read"]),
            ceiling(&["journal:read"]),
            ConversationContext::DirectMessage,
        ));

        assert!(decision.allowed);
        assert_eq!(decision.reason, DecisionReason::Allowed);
        assert_eq!(
            decision.grants[&name].obligations,
            BTreeSet::from([Obligation::DmOnly])
        );
    }
}
