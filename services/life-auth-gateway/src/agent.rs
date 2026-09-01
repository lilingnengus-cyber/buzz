//! Signed-source Life Agent turn delegation issuance and atomic consumption.

use crate::{
    call_grant::{CallGrantError, CallGrantInput, CallGrantSigner, SignedLifeCallGrant},
    catalog,
    iam::{
        authorize_in_transaction, AuthorizationError, AuthorizationRequest, ObligationSatisfaction,
    },
    identity::{ResolvedLifeIdentity, SessionPrincipal},
    membership::MembershipError,
    model::{AgentDelegationId, IdentityBindingId, LifeWorkbenchUserId, WorkbenchSessionId},
    Store,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration as ChronoDuration, Utc};
use life_iam::{
    Capability, CapabilityGrant, CapabilityRequest, ConversationContext, DataScope, EffectiveGrant,
    Obligation, ScopeSet,
};
use nostr::Event;
use rand::RngExt as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use uuid::Uuid;

const DELEGATION_AUDIENCE: &str = "life-workbench-mcp";
const SOURCE_MAX_AGE_SECONDS: u64 = 300;
const SOURCE_FUTURE_SKEW_SECONDS: u64 = 30;

/// Fixed policy for short-lived turn delegation credentials.
#[derive(Clone, Debug)]
pub struct DelegationPolicy {
    audience: String,
    ttl: Duration,
}

impl DelegationPolicy {
    /// Validates the fixed audience and a TTL between 30 and 900 seconds.
    pub fn new(audience: impl Into<String>, ttl: Duration) -> Result<Self, AgentError> {
        let audience = audience.into();
        if audience != DELEGATION_AUDIENCE || !(30..=900).contains(&ttl.as_secs()) {
            return Err(AgentError::Invalid);
        }
        Ok(Self { audience, ttl })
    }
}

/// Trusted conversation audience attached to the signed source event.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConversationAudience {
    /// One-to-one or small-group direct message.
    DirectMessage {
        /// Exact lower-case participant pubkeys, including the source author.
        participant_pubkeys: Vec<String>,
    },
    /// Channel message whose `h` tag must match `sourceChannelId`.
    Channel {
        /// Optional trusted participant snapshot; it never grants authority.
        participant_pubkeys: Vec<String>,
    },
}

/// Narrow requested scope format accepted from the trusted Pacioli host.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestedDataScope {
    /// Requested workspace identifiers.
    #[serde(default)]
    pub workspace: Vec<String>,
    /// Requested domain identifiers.
    #[serde(default)]
    pub domain: Vec<String>,
    /// Requested project identifiers.
    #[serde(default)]
    pub project: Vec<String>,
    /// Requested resource identifiers.
    #[serde(default)]
    pub resource: Vec<String>,
    /// Requested sensitivity classifications.
    #[serde(default)]
    pub sensitivity: Vec<String>,
    /// Requested operation-count buckets.
    #[serde(default)]
    pub operation_count: Vec<String>,
}

impl RequestedDataScope {
    pub(crate) fn from_data_scope(scope: &DataScope) -> Self {
        Self {
            workspace: values(&scope.workspaces),
            domain: values(&scope.domains),
            project: values(&scope.projects),
            resource: values(&scope.resources),
            sensitivity: values(&scope.sensitivities),
            operation_count: values(&scope.operation_count),
        }
    }
}

/// Exact resource context bound at issuance and consumption.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceContext {
    /// Fixed low-cardinality resource type.
    #[serde(rename = "type")]
    pub resource_type: String,
    /// Opaque resource identifier.
    pub id: String,
    /// Current optimistic version for mutation capabilities.
    pub expected_version: Option<i64>,
}

/// Pacioli-host request to authorize one signed source event and Agent turn.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IssueDelegationRequest {
    /// Complete signed Nostr source event.
    pub source_event: Event,
    /// Channel UUID, or `None` for a direct message.
    pub source_channel_id: Option<String>,
    /// Trusted conversation classification and participants.
    pub conversation: ConversationAudience,
    /// Stable Agent identifier.
    pub agent_id: String,
    /// Stable turn identifier.
    pub agent_turn_id: String,
    /// Requested catalog capabilities; custom values fail closed.
    pub requested_capabilities: Vec<String>,
    /// Requested data restrictions.
    pub requested_data_scope: RequestedDataScope,
    /// Optional exact resource bound to the turn.
    pub resource_context: Option<ResourceContext>,
    /// Future exact write-confirmation command identifier.
    pub write_command_id: Option<Uuid>,
    /// Distributed trace identifier.
    pub trace_id: Uuid,
}

/// One-time plaintext delegation credential and its non-secret envelope.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueDelegationResponse {
    /// Strong delegation identifier.
    pub delegation_id: AgentDelegationId,
    /// Random 32-byte Base64URL credential returned only once.
    pub token: String,
    /// Fixed MCP audience.
    pub audience: String,
    /// Effective capabilities after current IAM evaluation.
    pub effective_capabilities: Vec<Capability>,
    /// Common least-privilege data scope across effective capabilities.
    pub effective_data_scope: RequestedDataScope,
    /// Accumulated obligations carried by the delegation.
    pub obligations: BTreeSet<Obligation>,
    /// Atomic call budget.
    pub max_calls: i32,
    /// Hard credential expiration.
    pub expires_at: chrono::DateTime<Utc>,
    /// Immutable IAM decision identifier.
    pub iam_decision_id: Uuid,
    /// Distributed trace identifier.
    pub trace_id: Uuid,
}

/// MCP request to atomically consume one call from a delegation.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConsumeDelegationRequest {
    /// Exact Agent identifier bound at issuance.
    pub agent_id: String,
    /// Exact turn identifier bound at issuance.
    pub agent_turn_id: String,
    /// Fixed MCP tool name.
    pub tool: String,
    /// Catalog capability required by the tool.
    pub capability: String,
    /// Exact target resource.
    pub resource: ResourceContext,
    /// `sha256:` followed by 64 lower-case hexadecimal characters.
    pub normalized_input_hash: String,
    /// UUID idempotency key for this exact call.
    pub idempotency_key: String,
    /// Distributed trace identifier.
    pub trace_id: Uuid,
}

/// Stable fail-closed Agent delegation failure classes.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// Input shape, signature, kind, time window, or identifiers were invalid.
    #[error("Life Agent request is invalid")]
    Invalid,
    /// The opaque delegation credential or its bound runtime context was rejected.
    #[error("Life Agent delegation is unauthorized")]
    Unauthorized,
    /// Current IAM produced no effective permission.
    #[error("Life Agent authority is denied")]
    Denied,
    /// The source event or call idempotency key has already been consumed.
    #[error("Life Agent request conflicts with existing state")]
    Conflict,
    /// PostgreSQL could not complete the atomic transition.
    #[error("Life Agent authorization store unavailable")]
    Database,
    /// The one-call grant could not be signed.
    #[error("Life call grant unavailable")]
    Signing,
}

impl From<CallGrantError> for AgentError {
    fn from(_: CallGrantError) -> Self {
        Self::Signing
    }
}

impl From<AuthorizationError> for AgentError {
    fn from(value: AuthorizationError) -> Self {
        match value {
            AuthorizationError::Invalid => Self::Invalid,
            AuthorizationError::StaleAuthority => Self::Denied,
            AuthorizationError::Database => Self::Database,
        }
    }
}

impl From<MembershipError> for AgentError {
    fn from(value: MembershipError) -> Self {
        match value {
            MembershipError::Invalid => Self::Invalid,
            MembershipError::NotFound => Self::Unauthorized,
            MembershipError::Database => Self::Database,
        }
    }
}

impl Store {
    pub(crate) async fn delegation_identity(
        &self,
        source_pubkey: &str,
    ) -> Result<(LifeWorkbenchUserId, String, String, String), AgentError> {
        let row = sqlx::query(
            "SELECT u.id,u.oidc_issuer,u.oidc_subject,u.life_os_user_id
             FROM life_identity_bindings b
             JOIN life_workbench_users u ON u.id=b.workbench_user_id
             WHERE b.buzz_pubkey=$1 AND b.status='active' AND u.status='active'",
        )
        .bind(source_pubkey)
        .fetch_optional(&self.pool)
        .await
        .map_err(database)?
        .ok_or(AgentError::Unauthorized)?;
        Ok((
            LifeWorkbenchUserId::new(row.get("id")),
            row.get("oidc_issuer"),
            row.get("oidc_subject"),
            row.get("life_os_user_id"),
        ))
    }

    /// Atomically verifies a signed source, evaluates current IAM, and issues one delegation.
    pub async fn issue_agent_delegation(
        &self,
        request: IssueDelegationRequest,
        policy: &DelegationPolicy,
        current_identity: &ResolvedLifeIdentity,
    ) -> Result<IssueDelegationResponse, AgentError> {
        let context = validate_issue(&request)?;
        let (user_id, _, _, life_os_user_id) = self
            .delegation_identity(&request.source_event.pubkey.to_hex())
            .await?;
        if current_identity.life_os_user_id != life_os_user_id {
            self.mark_membership_sync_failed(user_id, request.trace_id)
                .await?;
            return Err(AgentError::Unauthorized);
        }
        self.refresh_membership_snapshot(
            &life_os_user_id,
            current_identity.active,
            &current_identity.memberships,
            request.trace_id,
        )
        .await?;
        if !current_identity.active {
            return Err(AgentError::Unauthorized);
        }
        let requested_scope = data_scope(&request.requested_data_scope)?;
        let requested = requested_capabilities(&request, &requested_scope)?;
        let runtime_ceiling = requested
            .keys()
            .cloned()
            .map(|capability| {
                (
                    capability,
                    CapabilityGrant {
                        data_scope: requested_scope.clone(),
                        obligations: BTreeSet::new(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut transaction = self.pool.begin().await.map_err(database)?;
        let binding = sqlx::query(
            "SELECT b.id,b.workbench_user_id,u.oidc_issuer,u.oidc_subject,u.life_os_user_id
             FROM life_identity_bindings b
             JOIN life_workbench_users u ON u.id=b.workbench_user_id
             WHERE b.buzz_pubkey=$1 AND b.status='active' AND u.status='active'
             FOR UPDATE OF b,u",
        )
        .bind(request.source_event.pubkey.to_hex())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?
        .ok_or(AgentError::Unauthorized)?;
        let user_id: Uuid = binding.get("workbench_user_id");
        let session = sqlx::query(
            "SELECT id,deployment_id FROM life_workbench_sessions
             WHERE workbench_user_id=$1 AND status='active' AND expires_at>now()
             ORDER BY created_at DESC LIMIT 1 FOR UPDATE",
        )
        .bind(user_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?
        .ok_or(AgentError::Unauthorized)?;
        let binding_id: Uuid = binding.get("id");
        let principal = SessionPrincipal {
            session_id: WorkbenchSessionId::new(session.get("id")),
            user_id: LifeWorkbenchUserId::new(user_id),
            issuer: binding.get("oidc_issuer"),
            subject: binding.get("oidc_subject"),
            life_os_user_id: binding.get("life_os_user_id"),
            deployment_id: session.get("deployment_id"),
        };
        let authorization = authorize_in_transaction(
            &mut transaction,
            AuthorizationRequest {
                principal: principal.clone(),
                identity_binding_id: IdentityBindingId::new(binding_id),
                agent_id: request.agent_id.clone(),
                agent_turn_id: request.agent_turn_id.clone(),
                source_event_id: Some(request.source_event.id.to_hex()),
                requested,
                runtime_ceiling,
                conversation: context,
                satisfaction: ObligationSatisfaction::default(),
                batch_size: 1,
                disclosure_allowed: false,
                trace_id: request.trace_id,
            },
        )
        .await?;
        if !authorization.decision.allowed {
            audit(
                &mut transaction,
                "LIFE_AGENT_TURN_DENIED",
                "denied",
                Some("iam_denied"),
                Some(user_id),
                None,
                Some(&request.agent_id),
                Some(&request.source_event.id.to_hex()),
                request.trace_id,
            )
            .await?;
            transaction.commit().await.map_err(database)?;
            return Err(AgentError::Denied);
        }
        let effective_capabilities = authorization
            .decision
            .allowed_capabilities
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let effective_data_scope = common_scope(&authorization.decision.grants)?;
        let obligations = common_obligations(&authorization.decision.grants);
        validate_resource_binding(
            &effective_capabilities,
            &effective_data_scope,
            request.resource_context.as_ref(),
        )?;
        let max_calls = if effective_capabilities
            .iter()
            .any(|capability| !capability.as_str().ends_with(":read"))
        {
            1
        } else {
            effective_capabilities
                .iter()
                .filter_map(|capability| catalog::capability(capability.as_str()))
                .map(|entry| entry.default_max_calls.min(20) as i32)
                .min()
                .unwrap_or(1)
        };
        let delegation_id = Uuid::new_v4();
        let token = random_token();
        let expires_at =
            Utc::now() + ChronoDuration::from_std(policy.ttl).map_err(|_| AgentError::Invalid)?;
        let insert = sqlx::query(
            "INSERT INTO life_agent_delegations
             (id,token_hash,workbench_user_id,workbench_session_id,identity_binding_id,
              principal_id,iam_decision_id,agent_id,agent_turn_id,source_event_id,
              source_pubkey,source_channel_id,conversation_context,audience,capabilities,
              data_scope,obligations,resource_context,write_command_id,catalog_version,
              status,expires_at,max_calls,remaining_calls,trace_id)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,
                    $18,$19,$20,'active',$21,$22,$22,$23)",
        )
        .bind(delegation_id)
        .bind(hash_token(&token))
        .bind(user_id)
        .bind(principal.session_id.as_uuid())
        .bind(binding_id)
        .bind(authorization.principal_id)
        .bind(authorization.decision_id)
        .bind(&request.agent_id)
        .bind(&request.agent_turn_id)
        .bind(request.source_event.id.to_hex())
        .bind(request.source_event.pubkey.to_hex())
        .bind(&request.source_channel_id)
        .bind(json(&request.conversation)?)
        .bind(&policy.audience)
        .bind(json(&effective_capabilities)?)
        .bind(json(&effective_data_scope)?)
        .bind(json(&obligations)?)
        .bind(request.resource_context.as_ref().map(json).transpose()?)
        .bind(request.write_command_id)
        .bind(authorization.catalog_version)
        .bind(expires_at)
        .bind(max_calls)
        .bind(request.trace_id)
        .execute(&mut *transaction)
        .await;
        if let Err(error) = insert {
            if error
                .as_database_error()
                .is_some_and(|error| error.is_unique_violation())
            {
                transaction.rollback().await.map_err(database)?;
                return Err(AgentError::Conflict);
            }
            return Err(AgentError::Database);
        }
        for event_type in ["LIFE_AGENT_TURN_GRANTED", "LIFE_DELEGATION_ISSUED"] {
            audit(
                &mut transaction,
                event_type,
                "success",
                None,
                Some(user_id),
                Some(delegation_id),
                Some(&request.agent_id),
                Some(&request.source_event.id.to_hex()),
                request.trace_id,
            )
            .await?;
        }
        transaction.commit().await.map_err(database)?;
        Ok(IssueDelegationResponse {
            delegation_id: AgentDelegationId::new(delegation_id),
            token,
            audience: policy.audience.clone(),
            effective_capabilities,
            effective_data_scope: RequestedDataScope::from_data_scope(&effective_data_scope),
            obligations,
            max_calls,
            expires_at,
            iam_decision_id: authorization.decision_id,
            trace_id: request.trace_id,
        })
    }

    /// Atomically checks every delegation binding, consumes budget, and signs one API call.
    pub async fn consume_agent_delegation(
        &self,
        token: &str,
        request: ConsumeDelegationRequest,
        signer: &CallGrantSigner,
    ) -> Result<SignedLifeCallGrant, AgentError> {
        validate_consume(token, &request)?;
        let input_hash = normalized_hash(&request.normalized_input_hash)?;
        let mut transaction = self.pool.begin().await.map_err(database)?;
        let row = sqlx::query(
            "SELECT d.id,d.agent_id,d.agent_turn_id,d.audience,d.capabilities,d.data_scope,
                    d.obligations,d.resource_context,d.conversation_context,d.status,
                    (d.expires_at>now()) AS delegation_current,d.max_calls,
                    d.remaining_calls,d.workbench_user_id,d.workbench_session_id,
                    u.status AS user_status,u.authority_sync_status,
                    b.status AS binding_status,s.status AS session_status,
                    (s.expires_at>now()) AS session_current,
                    p.status AS principal_status
             FROM life_agent_delegations d
             JOIN life_workbench_users u ON u.id=d.workbench_user_id
             JOIN life_identity_bindings b ON b.id=d.identity_binding_id
             JOIN life_workbench_sessions s ON s.id=d.workbench_session_id
             JOIN life_principals p ON p.id=d.principal_id
             WHERE d.token_hash=$1 FOR UPDATE OF d,u,b,s,p",
        )
        .bind(hash_token(token))
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?
        .ok_or(AgentError::Unauthorized)?;
        let delegation_id: Uuid = row.get("id");
        let status: String = row.get("status");
        if status == "active" && !row.get::<bool, _>("delegation_current") {
            sqlx::query("UPDATE life_agent_delegations SET status='expired',version=version+1 WHERE id=$1 AND status='active'")
                .bind(delegation_id).execute(&mut *transaction).await.map_err(database)?;
            audit(
                &mut transaction,
                "LIFE_DELEGATION_EXPIRED",
                "denied",
                Some("expired"),
                Some(row.get("workbench_user_id")),
                Some(delegation_id),
                Some(&request.agent_id),
                None,
                request.trace_id,
            )
            .await?;
            transaction.commit().await.map_err(database)?;
            return Err(AgentError::Unauthorized);
        }
        let capabilities: Vec<Capability> =
            serde_json::from_value(row.get("capabilities")).map_err(|_| AgentError::Database)?;
        let data_scope: DataScope =
            serde_json::from_value(row.get("data_scope")).map_err(|_| AgentError::Database)?;
        let obligations: BTreeSet<Obligation> =
            serde_json::from_value(row.get("obligations")).map_err(|_| AgentError::Database)?;
        let conversation: ConversationAudience =
            serde_json::from_value(row.get("conversation_context"))
                .map_err(|_| AgentError::Database)?;
        let stored_resource: Option<ResourceContext> = row
            .get::<Option<serde_json::Value>, _>("resource_context")
            .map(|value| serde_json::from_value(value).map_err(|_| AgentError::Database))
            .transpose()?;
        let capability =
            Capability::parse(request.capability.clone()).map_err(|_| AgentError::Invalid)?;
        let valid = status == "active"
            && row.get::<i32, _>("remaining_calls") > 0
            && row.get::<String, _>("audience") == DELEGATION_AUDIENCE
            && row.get::<String, _>("agent_id") == request.agent_id
            && row.get::<String, _>("agent_turn_id") == request.agent_turn_id
            && row.get::<String, _>("user_status") == "active"
            && row.get::<String, _>("authority_sync_status") == "current"
            && row.get::<String, _>("binding_status") == "active"
            && row.get::<String, _>("session_status") == "active"
            && row.get::<bool, _>("session_current")
            && row.get::<String, _>("principal_status") == "active"
            && capabilities.contains(&capability)
            && catalog::tool(&request.tool)
                .is_some_and(|tool| tool.capability == request.capability)
            && stored_resource
                .as_ref()
                .is_none_or(|resource| resource == &request.resource)
            && resource_allowed(&data_scope, &request.resource.id)
            && expected_version_allowed(&request.capability, &request.resource)
            && obligations_satisfied(&obligations, &conversation);
        if !valid {
            return Err(AgentError::Unauthorized);
        }
        let call_id = Uuid::new_v4();
        let inserted = sqlx::query(
            "INSERT INTO life_delegation_calls
             (id,delegation_id,call_id,capability,normalized_input_hash,idempotency_key,
              resource_type,resource_id,expected_version,status,trace_id)
             VALUES($1,$2,$1,$3,$4,$5,$6,$7,$8,'issued',$9)",
        )
        .bind(call_id)
        .bind(delegation_id)
        .bind(&request.capability)
        .bind(input_hash)
        .bind(&request.idempotency_key)
        .bind(&request.resource.resource_type)
        .bind(&request.resource.id)
        .bind(request.resource.expected_version)
        .bind(request.trace_id)
        .execute(&mut *transaction)
        .await;
        if let Err(error) = inserted {
            if error
                .as_database_error()
                .is_some_and(|error| error.is_unique_violation())
            {
                return Err(AgentError::Conflict);
            }
            return Err(AgentError::Database);
        }
        let remaining = row.get::<i32, _>("remaining_calls") - 1;
        sqlx::query(
            "UPDATE life_agent_delegations
             SET remaining_calls=$2,status=CASE WHEN $2=0 THEN 'exhausted' ELSE 'active' END,
                 last_used_at=now(),version=version+1 WHERE id=$1",
        )
        .bind(delegation_id)
        .bind(remaining)
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        audit(
            &mut transaction,
            "LIFE_DELEGATION_CONSUMED",
            "success",
            None,
            Some(row.get("workbench_user_id")),
            Some(delegation_id),
            Some(&request.agent_id),
            None,
            request.trace_id,
        )
        .await?;
        if remaining == 0 {
            audit(
                &mut transaction,
                "LIFE_DELEGATION_EXHAUSTED",
                "success",
                None,
                Some(row.get("workbench_user_id")),
                Some(delegation_id),
                Some(&request.agent_id),
                None,
                request.trace_id,
            )
            .await?;
        }
        let grant = signer.issue(CallGrantInput {
            delegation_id,
            call_id,
            capability: &request.capability,
            data_scope,
            resource: &request.resource,
            normalized_input_hash: &request.normalized_input_hash,
            idempotency_key: &request.idempotency_key,
            trace_id: request.trace_id,
        })?;
        transaction.commit().await.map_err(database)?;
        Ok(grant)
    }
}

fn validate_issue(request: &IssueDelegationRequest) -> Result<ConversationContext, AgentError> {
    let event = &request.source_event;
    let now = u64::try_from(Utc::now().timestamp()).map_err(|_| AgentError::Invalid)?;
    let created = event.created_at.as_secs();
    let valid_kind = matches!(event.kind.as_u16(), 1 | 9 | 40002 | 45001 | 45003);
    if !event.verify_id()
        || !event.verify_signature()
        || !valid_kind
        || created.saturating_add(SOURCE_MAX_AGE_SECONDS) < now
        || created > now.saturating_add(SOURCE_FUTURE_SKEW_SECONDS)
        || !safe_id(&request.agent_id, 512)
        || !safe_id(&request.agent_turn_id, 512)
    {
        return Err(AgentError::Invalid);
    }
    match (&request.conversation, &request.source_channel_id) {
        (
            ConversationAudience::Channel {
                participant_pubkeys,
            },
            Some(channel),
        ) => {
            let channels = event_tag_values(event, "h");
            let participants = participant_pubkeys.iter().collect::<BTreeSet<_>>();
            if Uuid::parse_str(channel).is_err()
                || participant_pubkeys.len() > 10_000
                || participants.len() != participant_pubkeys.len()
                || participant_pubkeys.iter().any(|value| !valid_pubkey(value))
                || channels.as_slice() != [channel.as_str()]
            {
                return Err(AgentError::Invalid);
            }
            Ok(ConversationContext::MultiPartyChannel)
        }
        (
            ConversationAudience::DirectMessage {
                participant_pubkeys,
            },
            None,
        ) => {
            let participants = participant_pubkeys.iter().collect::<BTreeSet<_>>();
            let author = event.pubkey.to_hex();
            let recipients = event_tag_values(event, "p");
            let recipient_set = recipients.iter().copied().collect::<BTreeSet<_>>();
            let expected_recipients = participants
                .iter()
                .filter(|value| value.as_str() != author)
                .map(|value| value.as_str())
                .collect::<BTreeSet<_>>();
            if !(2..=9).contains(&participant_pubkeys.len())
                || participants.len() != participant_pubkeys.len()
                || participant_pubkeys.iter().any(|value| !valid_pubkey(value))
                || !participants.contains(&author)
                || recipients.len() != recipient_set.len()
                || recipient_set != expected_recipients
                || !event_tag_values(event, "h").is_empty()
            {
                return Err(AgentError::Invalid);
            }
            Ok(ConversationContext::DirectMessage)
        }
        _ => Err(AgentError::Invalid),
    }
}

fn requested_capabilities(
    request: &IssueDelegationRequest,
    scope: &DataScope,
) -> Result<BTreeMap<Capability, CapabilityRequest>, AgentError> {
    if request.requested_capabilities.is_empty() || request.requested_capabilities.len() > 64 {
        return Err(AgentError::Invalid);
    }
    let mut requested = BTreeMap::new();
    for name in &request.requested_capabilities {
        let capability = Capability::parse(name.clone()).map_err(|_| AgentError::Invalid)?;
        let entry = catalog::capability(name).ok_or(AgentError::Denied)?;
        if entry.requires_expected_version
            && request
                .resource_context
                .as_ref()
                .and_then(|resource| resource.expected_version)
                .is_none()
        {
            return Err(AgentError::Invalid);
        }
        if requested
            .insert(
                capability,
                CapabilityRequest {
                    data_scope: scope.clone(),
                    obligations: BTreeSet::new(),
                },
            )
            .is_some()
        {
            return Err(AgentError::Invalid);
        }
    }
    Ok(requested)
}

fn data_scope(request: &RequestedDataScope) -> Result<DataScope, AgentError> {
    Ok(DataScope {
        workspaces: scope(&request.workspace)?,
        domains: scope(&request.domain)?,
        projects: scope(&request.project)?,
        resources: scope(&request.resource)?,
        sensitivities: scope(&request.sensitivity)?,
        operation_count: scope(&request.operation_count)?,
    })
}

fn scope(values: &[String]) -> Result<ScopeSet, AgentError> {
    if values.is_empty() {
        Ok(ScopeSet::Unrestricted)
    } else if values.len() <= 10_000 {
        ScopeSet::restricted(values.iter().cloned()).map_err(|_| AgentError::Invalid)
    } else {
        Err(AgentError::Invalid)
    }
}

fn values(scope: &ScopeSet) -> Vec<String> {
    match scope {
        ScopeSet::Unrestricted => Vec::new(),
        ScopeSet::Restricted(values) => values.iter().cloned().collect(),
    }
}

fn common_scope(grants: &BTreeMap<Capability, EffectiveGrant>) -> Result<DataScope, AgentError> {
    let mut grants = grants.values();
    let mut scope = grants.next().ok_or(AgentError::Denied)?.data_scope.clone();
    for grant in grants {
        scope = scope
            .intersection(&grant.data_scope)
            .ok_or(AgentError::Denied)?;
    }
    Ok(scope)
}

fn common_obligations(grants: &BTreeMap<Capability, EffectiveGrant>) -> BTreeSet<Obligation> {
    let mut obligations = BTreeSet::new();
    let mut max_batch = None;
    for obligation in grants.values().flat_map(|grant| &grant.obligations) {
        match obligation {
            Obligation::MaxBatch(limit) => {
                max_batch = Some(max_batch.map_or(*limit, |current: u32| current.min(*limit)))
            }
            obligation => {
                obligations.insert(obligation.clone());
            }
        }
    }
    if let Some(limit) = max_batch {
        obligations.insert(Obligation::MaxBatch(limit));
    }
    obligations
}

fn validate_resource_binding(
    capabilities: &[Capability],
    scope: &DataScope,
    resource: Option<&ResourceContext>,
) -> Result<(), AgentError> {
    if let Some(resource) = resource {
        if !safe_id(&resource.resource_type, 128)
            || !safe_text(&resource.id, 512)
            || resource.expected_version.is_some_and(|version| version < 0)
            || !resource_allowed(scope, &resource.id)
            || capabilities
                .iter()
                .any(|capability| !expected_version_allowed(capability.as_str(), resource))
        {
            return Err(AgentError::Invalid);
        }
    } else if capabilities.iter().any(|capability| {
        catalog::capability(capability.as_str())
            .is_some_and(|entry| entry.requires_expected_version)
    }) {
        return Err(AgentError::Invalid);
    }
    Ok(())
}

fn validate_consume(token: &str, request: &ConsumeDelegationRequest) -> Result<(), AgentError> {
    if URL_SAFE_NO_PAD
        .decode(token)
        .ok()
        .is_none_or(|bytes| bytes.len() != 32)
        || !safe_id(&request.agent_id, 512)
        || !safe_id(&request.agent_turn_id, 512)
        || !safe_id(&request.tool, 128)
        || !safe_id(&request.capability, 128)
        || Uuid::parse_str(&request.idempotency_key).is_err()
        || !safe_id(&request.resource.resource_type, 128)
        || !safe_text(&request.resource.id, 512)
    {
        return Err(AgentError::Invalid);
    }
    Ok(())
}

fn normalized_hash(value: &str) -> Result<Vec<u8>, AgentError> {
    let encoded = value.strip_prefix("sha256:").ok_or(AgentError::Invalid)?;
    if encoded.len() != 64
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(AgentError::Invalid);
    }
    hex::decode(encoded).map_err(|_| AgentError::Invalid)
}

fn expected_version_allowed(capability: &str, resource: &ResourceContext) -> bool {
    catalog::capability(capability).is_some_and(|entry| {
        !entry.requires_expected_version
            || resource
                .expected_version
                .is_some_and(|version| version >= 0)
    })
}

fn resource_allowed(scope: &DataScope, resource_id: &str) -> bool {
    match &scope.resources {
        ScopeSet::Unrestricted => true,
        ScopeSet::Restricted(values) => values.contains(resource_id),
    }
}

fn obligations_satisfied(
    obligations: &BTreeSet<Obligation>,
    conversation: &ConversationAudience,
) -> bool {
    obligations.iter().all(|obligation| match obligation {
        Obligation::MaxBatch(limit) => *limit >= 1,
        Obligation::DmOnly => matches!(conversation, ConversationAudience::DirectMessage { .. }),
        Obligation::HumanConfirmation
        | Obligation::StepUpAuthentication
        | Obligation::DualControl
        | Obligation::RedactSensitive => true,
    })
}

fn event_tag_values<'a>(event: &'a Event, name: &str) -> Vec<&'a str> {
    event
        .tags
        .iter()
        .filter_map(|tag| {
            let values = tag.as_slice();
            (values.first().is_some_and(|value| value == name))
                .then(|| values.get(1).map(String::as_str))
                .flatten()
        })
        .collect()
}

fn valid_pubkey(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn safe_id(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

fn safe_text(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn json<T: Serialize>(value: &T) -> Result<serde_json::Value, AgentError> {
    serde_json::to_value(value).map_err(|_| AgentError::Database)
}

#[allow(clippy::too_many_arguments)]
async fn audit(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    event_type: &str,
    outcome: &str,
    reason: Option<&str>,
    user_id: Option<Uuid>,
    delegation_id: Option<Uuid>,
    agent_id: Option<&str>,
    source_event_id: Option<&str>,
    trace_id: Uuid,
) -> Result<(), AgentError> {
    sqlx::query(
        "INSERT INTO life_security_audit
         (event_type,outcome,reason_code,subject_kind,subject_id,workbench_user_id,
          delegation_id,source_event_id,trace_id)
         VALUES($1,$2,$3,'agent',$4,$5,$6,$7,$8)",
    )
    .bind(event_type)
    .bind(outcome)
    .bind(reason)
    .bind(agent_id)
    .bind(user_id)
    .bind(delegation_id)
    .bind(source_event_id)
    .bind(trace_id)
    .execute(&mut **transaction)
    .await
    .map_err(database)?;
    Ok(())
}

fn database(_: sqlx::Error) -> AgentError {
    AgentError::Database
}
