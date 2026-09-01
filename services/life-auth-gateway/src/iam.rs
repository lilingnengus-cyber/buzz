//! Transactional resolution and persistence around the pure Life IAM evaluator.

use crate::{
    catalog::{self, CatalogEntry, RiskClass, CATALOG_VERSION},
    identity::SessionPrincipal,
    model::IdentityBindingId,
    Store,
};
use life_iam::{
    evaluate, Authority, Capability, CapabilityGrant, CapabilityRequest, ConversationContext,
    DataScope, Decision, DecisionReason, EvaluationInput, Obligation, ScopeSet,
};
use serde::Serialize;
use sqlx::{Postgres, Row, Transaction};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Trusted evidence that runtime obligations have been fulfilled.
#[derive(Clone, Debug, Default)]
pub struct ObligationSatisfaction {
    /// A human confirmed the exact proposed operation.
    pub human_confirmation: bool,
    /// The human completed fresh higher-assurance authentication.
    pub step_up_authentication: bool,
    /// A second authorized human approved the operation.
    pub dual_control: bool,
    /// The result path guarantees sensitive-field redaction.
    pub redact_sensitive: bool,
}

/// Fully bound request for one immutable authorization transaction.
#[derive(Clone, Debug)]
pub struct AuthorizationRequest {
    /// Current authenticated Workbench session.
    pub principal: SessionPrincipal,
    /// Active Nostr binding that authorized the source event.
    pub identity_binding_id: IdentityBindingId,
    /// Agent identity; a matching independent principal never falls back to the human.
    pub agent_id: String,
    /// Stable Agent turn identifier.
    pub agent_turn_id: String,
    /// Verified lower-case Nostr source event identifier when applicable.
    pub source_event_id: Option<String>,
    /// Capabilities and scopes requested by the turn.
    pub requested: BTreeMap<Capability, CapabilityRequest>,
    /// Trusted Pacioli host-policy ceiling.
    pub runtime_ceiling: BTreeMap<Capability, CapabilityGrant>,
    /// Verified DM or multi-party channel classification.
    pub conversation: ConversationContext,
    /// Trusted evidence for explicit obligations.
    pub satisfaction: ObligationSatisfaction,
    /// Number of resources affected by this operation.
    pub batch_size: u32,
    /// Channel disclosure state, recorded by the caller but never used to grant authority.
    pub disclosure_allowed: bool,
    /// Low-sensitivity distributed trace identifier.
    pub trace_id: Uuid,
}

/// Persisted outcome of one Life authorization transaction.
#[derive(Clone, Debug)]
pub struct AuthorizationOutcome {
    /// Immutable decision record identifier.
    pub decision_id: Uuid,
    /// Human or independent-Agent principal selected for this decision.
    pub principal_id: Uuid,
    /// Pure evaluator decision after obligation satisfaction gates.
    pub decision: Decision,
    /// Catalog version used for this decision.
    pub catalog_version: i32,
}

/// Stable fail-closed authorization failure classes.
#[derive(Debug, thiserror::Error)]
pub enum AuthorizationError {
    /// Session, binding, source identifiers, or request bounds were invalid.
    #[error("Life authorization context is invalid")]
    Invalid,
    /// The LifeOS authority mirror is stale and cannot authorize new work.
    #[error("Life authorization snapshot is stale")]
    StaleAuthority,
    /// PostgreSQL could not complete the atomic decision.
    #[error("Life authorization store unavailable")]
    Database,
}

impl Store {
    /// Resolves current authority, evaluates least privilege, and appends one decision atomically.
    pub async fn authorize(
        &self,
        request: AuthorizationRequest,
    ) -> Result<AuthorizationOutcome, AuthorizationError> {
        let mut transaction = self.pool.begin().await.map_err(database)?;
        let outcome = authorize_in_transaction(&mut transaction, request).await?;
        transaction.commit().await.map_err(database)?;
        Ok(outcome)
    }
}

pub(crate) async fn authorize_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    request: AuthorizationRequest,
) -> Result<AuthorizationOutcome, AuthorizationError> {
    validate_request(&request)?;
    validate_context(transaction, &request).await?;
    validate_current_catalog(transaction).await?;
    let human_principal_id =
        ensure_human_principal(transaction, request.principal.user_id.as_uuid()).await?;
    let known = request
        .requested
        .keys()
        .all(|capability| catalog::capability(capability.as_str()).is_some());
    let (selected_principal, mut decision) = if !known {
        (
            human_principal_id,
            deny_all(&request.requested, DecisionReason::NoEffectivePermission),
        )
    } else if let Some(agent) = independent_authority(transaction, &request.agent_id).await? {
        let principal_id =
            Uuid::parse_str(&agent.principal_id).map_err(|_| AuthorizationError::Database)?;
        let input = EvaluationInput::independent_agent(
            agent,
            request.requested.clone(),
            request.runtime_ceiling.clone(),
            request.conversation,
        );
        (principal_id, evaluate(input))
    } else {
        let authority = human_authority(
            transaction,
            human_principal_id,
            request.principal.user_id.as_uuid(),
        )
        .await?;
        let input = EvaluationInput::proxy_agent(
            authority,
            request.requested.clone(),
            request.runtime_ceiling.clone(),
            request.conversation,
        );
        (human_principal_id, evaluate(input))
    };
    enforce_satisfaction(&mut decision, &request.satisfaction, request.batch_size);
    let decision_id =
        persist_decision(transaction, selected_principal, &request, &decision).await?;
    Ok(AuthorizationOutcome {
        decision_id,
        principal_id: selected_principal,
        decision,
        catalog_version: CATALOG_VERSION,
    })
}

async fn validate_current_catalog(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), AuthorizationError> {
    let capabilities = sqlx::query_scalar::<_, String>(
        "SELECT capability FROM life_capability_catalog
         WHERE status='active' AND catalog_version=$1 ORDER BY capability",
    )
    .bind(CATALOG_VERSION)
    .fetch_all(&mut **transaction)
    .await
    .map_err(database)?;
    let expected = catalog::entries()
        .iter()
        .map(|entry| entry.capability)
        .collect::<BTreeSet<_>>();
    if capabilities.len() != expected.len()
        || capabilities
            .iter()
            .any(|name| !expected.contains(name.as_str()))
    {
        return Err(AuthorizationError::Invalid);
    }
    Ok(())
}

async fn validate_context(
    transaction: &mut Transaction<'_, Postgres>,
    request: &AuthorizationRequest,
) -> Result<(), AuthorizationError> {
    let row = sqlx::query(
        "SELECT u.authority_sync_status
         FROM life_workbench_sessions s
         JOIN life_workbench_users u ON u.id=s.workbench_user_id
         JOIN life_identity_bindings b ON b.workbench_user_id=u.id
         WHERE s.id=$1 AND s.workbench_user_id=$2 AND s.deployment_id=$3
           AND s.status='active' AND s.expires_at>now() AND u.status='active'
           AND b.id=$4 AND b.status='active'",
    )
    .bind(request.principal.session_id.as_uuid())
    .bind(request.principal.user_id.as_uuid())
    .bind(&request.principal.deployment_id)
    .bind(request.identity_binding_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database)?
    .ok_or(AuthorizationError::Invalid)?;
    if row.get::<String, _>("authority_sync_status") != "current" {
        return Err(AuthorizationError::StaleAuthority);
    }
    Ok(())
}

async fn ensure_human_principal(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<Uuid, AuthorizationError> {
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM life_principals
         WHERE workbench_user_id=$1 AND kind='human' AND status='active' FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database)?
    {
        return Ok(id);
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO life_principals(id,workbench_user_id,kind,status)
         VALUES($1,$2,'human','active')",
    )
    .bind(id)
    .bind(user_id)
    .execute(&mut **transaction)
    .await
    .map_err(database)?;
    Ok(id)
}

async fn human_authority(
    transaction: &mut Transaction<'_, Postgres>,
    principal_id: Uuid,
    user_id: Uuid,
) -> Result<Authority, AuthorizationError> {
    let memberships = sqlx::query(
        "SELECT workspace_id,role_code FROM life_workspace_memberships
         WHERE workbench_user_id=$1 AND status='active'",
    )
    .bind(user_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(database)?;
    let mut scopes = BTreeMap::<&'static str, BTreeSet<String>>::new();
    for row in memberships {
        let workspace: String = row.get("workspace_id");
        let role: String = row.get("role_code");
        for entry in catalog::entries()
            .iter()
            .filter(|entry| role_allows(&role, entry))
        {
            scopes
                .entry(entry.capability)
                .or_default()
                .insert(workspace.clone());
        }
    }
    let grants = scopes
        .into_iter()
        .map(|(name, workspaces)| {
            let entry = catalog::capability(name).expect("compiled catalog lookup");
            Ok((
                Capability::parse(name).map_err(|_| AuthorizationError::Database)?,
                grant(
                    entry,
                    ScopeSet::restricted(workspaces).map_err(|_| AuthorizationError::Database)?,
                ),
            ))
        })
        .collect::<Result<BTreeMap<_, _>, AuthorizationError>>()?;
    Ok(Authority {
        principal_id: principal_id.to_string(),
        active: true,
        grants,
    })
}

fn role_allows(role: &str, entry: &CatalogEntry) -> bool {
    match role {
        "OWNER" | "ADMIN" => true,
        "MEMBER" => entry.risk_class != RiskClass::High,
        "VIEWER" => entry.capability.ends_with(":read"),
        _ => false,
    }
}

async fn independent_authority(
    transaction: &mut Transaction<'_, Postgres>,
    agent_id: &str,
) -> Result<Option<Authority>, AuthorizationError> {
    let rows = sqlx::query(
        "SELECT id,status FROM life_principals
         WHERE agent_id=$1 AND kind='independent_agent'
         ORDER BY CASE status WHEN 'active' THEN 0 ELSE 1 END,created_at DESC FOR UPDATE",
    )
    .bind(agent_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(database)?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    let principal_id: Uuid = row.get("id");
    let active = row.get::<String, _>("status") == "active";
    let mut grants = BTreeMap::new();
    if active {
        for row in sqlx::query(
            "SELECT capability,data_scope,obligations FROM life_principal_capabilities
             WHERE principal_id=$1 AND status='active' AND catalog_version=$2",
        )
        .bind(principal_id)
        .bind(CATALOG_VERSION)
        .fetch_all(&mut **transaction)
        .await
        .map_err(database)?
        {
            let name: String = row.get("capability");
            let Some(entry) = catalog::capability(&name) else {
                continue;
            };
            let data_scope: DataScope = serde_json::from_value(row.get("data_scope"))
                .map_err(|_| AuthorizationError::Database)?;
            let mut capability_grant = grant(entry, data_scope.workspaces.clone());
            capability_grant.data_scope = data_scope;
            for obligation in parse_obligations(row.get("obligations"))? {
                capability_grant.obligations.insert(obligation);
            }
            grants.insert(
                Capability::parse(name).map_err(|_| AuthorizationError::Database)?,
                capability_grant,
            );
        }
    }
    Ok(Some(Authority {
        principal_id: principal_id.to_string(),
        active,
        grants,
    }))
}

fn grant(entry: &CatalogEntry, workspaces: ScopeSet) -> CapabilityGrant {
    let mut obligations = entry
        .obligations
        .iter()
        .filter_map(|name| obligation(name))
        .collect::<BTreeSet<_>>();
    obligations.insert(Obligation::MaxBatch(entry.max_batch_size));
    CapabilityGrant {
        data_scope: DataScope {
            workspaces,
            ..DataScope::default()
        },
        obligations,
    }
}

fn parse_obligations(value: serde_json::Value) -> Result<BTreeSet<Obligation>, AuthorizationError> {
    let names: Vec<String> =
        serde_json::from_value(value).map_err(|_| AuthorizationError::Database)?;
    names
        .iter()
        .map(|name| obligation(name).ok_or(AuthorizationError::Database))
        .collect()
}

fn obligation(name: &str) -> Option<Obligation> {
    match name {
        "human_confirmation" => Some(Obligation::HumanConfirmation),
        "step_up_authentication" => Some(Obligation::StepUpAuthentication),
        "dual_control" => Some(Obligation::DualControl),
        "dm_only" => Some(Obligation::DmOnly),
        "redact_sensitive" => Some(Obligation::RedactSensitive),
        _ => None,
    }
}

fn enforce_satisfaction(
    decision: &mut Decision,
    satisfaction: &ObligationSatisfaction,
    batch_size: u32,
) {
    let denied = decision
        .grants
        .iter()
        .filter_map(|(capability, grant)| {
            let unmet = grant.obligations.iter().any(|obligation| match obligation {
                Obligation::HumanConfirmation => !satisfaction.human_confirmation,
                Obligation::StepUpAuthentication => !satisfaction.step_up_authentication,
                Obligation::DualControl => !satisfaction.dual_control,
                Obligation::RedactSensitive => !satisfaction.redact_sensitive,
                Obligation::MaxBatch(limit) => batch_size == 0 || batch_size > *limit,
                Obligation::DmOnly => false,
            });
            unmet.then_some(capability.clone())
        })
        .collect::<Vec<_>>();
    for capability in denied {
        decision.grants.remove(&capability);
        decision.allowed_capabilities.remove(&capability);
        decision.denied_capabilities.insert(capability);
    }
    decision.allowed = !decision.allowed_capabilities.is_empty();
    decision.reason = match (decision.allowed, decision.denied_capabilities.is_empty()) {
        (true, true) => DecisionReason::Allowed,
        (true, false) => DecisionReason::PartiallyAllowed,
        (false, _) => DecisionReason::NoEffectivePermission,
    };
}

fn deny_all(
    requested: &BTreeMap<Capability, CapabilityRequest>,
    reason: DecisionReason,
) -> Decision {
    Decision {
        allowed: false,
        reason,
        allowed_capabilities: BTreeSet::new(),
        denied_capabilities: requested.keys().cloned().collect(),
        grants: BTreeMap::new(),
    }
}

async fn persist_decision(
    transaction: &mut Transaction<'_, Postgres>,
    principal_id: Uuid,
    request: &AuthorizationRequest,
    decision: &Decision,
) -> Result<Uuid, AuthorizationError> {
    let id = Uuid::new_v4();
    let requested = request
        .requested
        .keys()
        .map(Capability::as_str)
        .collect::<Vec<_>>();
    sqlx::query(
        "INSERT INTO life_iam_decisions
         (id,principal_id,workbench_user_id,agent_id,agent_turn_id,source_event_id,
          requested_capabilities,effective_grants,denied_capabilities,decision_reason,
          catalog_version,trace_id)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind(id)
    .bind(principal_id)
    .bind(request.principal.user_id.as_uuid())
    .bind(&request.agent_id)
    .bind(&request.agent_turn_id)
    .bind(&request.source_event_id)
    .bind(json(&requested)?)
    .bind(json(&decision.grants)?)
    .bind(json(&decision.denied_capabilities)?)
    .bind(reason(decision.reason))
    .bind(CATALOG_VERSION)
    .bind(request.trace_id)
    .execute(&mut **transaction)
    .await
    .map_err(database)?;
    Ok(id)
}

fn json(value: &impl Serialize) -> Result<serde_json::Value, AuthorizationError> {
    serde_json::to_value(value).map_err(|_| AuthorizationError::Database)
}

fn reason(reason: DecisionReason) -> &'static str {
    match reason {
        DecisionReason::Allowed => "allowed",
        DecisionReason::PartiallyAllowed => "partially_allowed",
        DecisionReason::NoEffectivePermission => "no_effective_permission",
        DecisionReason::SubjectInactive => "subject_inactive",
        DecisionReason::EmptyRequest => "empty_request",
    }
}

fn validate_request(request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
    let safe = |value: &str, max| {
        !value.is_empty()
            && value.len() <= max
            && value.trim() == value
            && !value.chars().any(char::is_control)
    };
    if !safe(&request.agent_id, 512)
        || !safe(&request.agent_turn_id, 512)
        || request.requested.len() > 256
        || request.runtime_ceiling.len() > 256
        || request.source_event_id.as_ref().is_some_and(|id| {
            id.len() != 64
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
    {
        return Err(AuthorizationError::Invalid);
    }
    Ok(())
}

fn database(_: sqlx::Error) -> AuthorizationError {
    AuthorizationError::Database
}
