use crate::{
    auth::OidcIdentity,
    model::{
        IdentityBindingChallengeId, IdentityBindingId, LifeWorkbenchUserId, WorkbenchSessionId,
    },
    security::OutboundServiceCredential,
    store::Store,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use nostr::{Event, Kind};
use rand::Rng as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction};
use url::Url;
use uuid::Uuid;

const BINDING_AUDIENCE: &str = "life-workbench-identity-binding";

/// One current LifeOS workspace membership returned by the identity source.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedMembership {
    /// Opaque LifeOS workspace identifier.
    pub workspace_id: String,
    /// Current LifeOS role code.
    pub role: String,
    /// Monotonic membership version supplied by LifeOS.
    pub membership_version: i64,
}

/// Explicit `(issuer, subject)` mapping returned by LifeOS.
#[derive(Clone, Debug)]
pub struct ResolvedLifeIdentity {
    /// Canonical opaque LifeOS user identifier.
    pub life_os_user_id: String,
    /// Whether LifeOS currently permits Workbench access.
    pub active: bool,
    /// Current workspace membership snapshot.
    pub memberships: Vec<ResolvedMembership>,
}

/// Stable identity workflow failure classes safe for HTTP mapping.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    /// A required service or session credential was absent or invalid.
    #[error("identity request is unauthorized")]
    Unauthorized,
    /// Request syntax, bounds, signature, or canonical content was invalid.
    #[error("identity request is invalid")]
    Invalid,
    /// LifeOS has no explicit mapping for the verified issuer and subject.
    #[error("identity is not explicitly mapped")]
    NotMapped,
    /// The mapped user or local identity mirror is disabled.
    #[error("identity is inactive")]
    Inactive,
    /// The requested identity transition conflicts with active state.
    #[error("identity state conflicts with an active record")]
    Conflict,
    /// The requested owned identity record does not exist.
    #[error("identity record was not found")]
    NotFound,
    /// PostgreSQL, LifeOS, or OIDC infrastructure was unavailable.
    #[error("identity dependency unavailable")]
    Unavailable,
}

/// Client for the one fixed LifeOS external-identity resolution route.
#[derive(Clone)]
pub struct LifeOsIdentityClient {
    client: reqwest::Client,
    endpoint: Url,
    credential: OutboundServiceCredential,
}

impl LifeOsIdentityClient {
    pub(crate) fn new(
        base_url: &Url,
        credential: &OutboundServiceCredential,
    ) -> Result<Self, IdentityError> {
        let endpoint = base_url
            .join("api/internal/workbench-identities/resolve")
            .map_err(|_| IdentityError::Unavailable)?;
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(2))
            .timeout(std::time::Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| IdentityError::Unavailable)?;
        Ok(Self {
            client,
            endpoint,
            credential: credential.clone(),
        })
    }

    pub(crate) async fn resolve(
        &self,
        issuer: &str,
        subject: &str,
    ) -> Result<ResolvedLifeIdentity, IdentityError> {
        #[derive(Serialize)]
        struct Request<'a> {
            issuer: &'a str,
            subject: &'a str,
        }
        #[derive(Deserialize)]
        struct User {
            id: String,
            active: bool,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            ok: bool,
            user: User,
            memberships: Vec<ResolvedMembership>,
        }

        let response = self
            .client
            .post(self.endpoint.clone())
            .bearer_auth(self.credential.expose())
            .json(&Request { issuer, subject })
            .send()
            .await
            .map_err(|_| IdentityError::Unavailable)?;
        match response.status() {
            reqwest::StatusCode::NOT_FOUND => return Err(IdentityError::NotMapped),
            reqwest::StatusCode::FORBIDDEN => return Err(IdentityError::Inactive),
            reqwest::StatusCode::UNAUTHORIZED => return Err(IdentityError::Unavailable),
            status if !status.is_success() => return Err(IdentityError::Unavailable),
            _ => {}
        }
        let response = response
            .json::<Response>()
            .await
            .map_err(|_| IdentityError::Unavailable)?;
        if !response.ok || response.user.id.is_empty() {
            return Err(IdentityError::NotMapped);
        }
        Ok(ResolvedLifeIdentity {
            life_os_user_id: response.user.id,
            active: response.user.active,
            memberships: response.memberships,
        })
    }
}

/// Opaque session minted after OIDC verification and explicit LifeOS mapping.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuedSession {
    /// Strong identifier for the persisted Workbench Session.
    pub session_id: WorkbenchSessionId,
    /// Single returned bearer whose SHA-256 digest is persisted.
    pub session_token: String,
    /// Session expiry, bounded by the verified OIDC token.
    pub expires_at: DateTime<Utc>,
    /// Gateway-local mapped user identifier.
    pub user_id: LifeWorkbenchUserId,
    /// Canonical opaque LifeOS user identifier.
    pub life_os_user_id: String,
}

/// Authenticated context recovered from a hash-only Workbench Session.
#[derive(Clone, Debug)]
pub struct SessionPrincipal {
    /// Authenticated Workbench Session identifier.
    pub session_id: WorkbenchSessionId,
    /// Gateway-local mapped user identifier.
    pub user_id: LifeWorkbenchUserId,
    /// Exact verified OIDC issuer.
    pub issuer: String,
    /// Exact issuer-local opaque subject.
    pub subject: String,
    /// Canonical opaque LifeOS user identifier.
    pub life_os_user_id: String,
    /// Deployment to which the session is bound.
    pub deployment_id: String,
}

/// One-time canonical challenge signed by the requested Nostr pubkey.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingChallenge {
    /// One-time challenge identifier.
    pub challenge_id: IdentityBindingChallengeId,
    /// Fixed Life identity-binding audience.
    pub audience: &'static str,
    /// Exact text that kind `24243` must sign.
    pub canonical_payload: String,
    /// Challenge expiration instant.
    pub expires_at: DateTime<Utc>,
    /// Low-sensitivity correlation identifier.
    pub trace_id: Uuid,
}

/// Active identity binding summary; device metadata is deliberately absent.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityBinding {
    /// Strong identifier for the binding.
    pub binding_id: IdentityBindingId,
    /// Bound lower-case hexadecimal Nostr public key.
    pub pubkey: String,
    /// Current binding lifecycle status.
    pub status: String,
    /// Binding creation instant.
    pub created_at: DateTime<Utc>,
    /// Optimistic lifecycle version.
    pub version: i64,
}

/// Current account/session view returned without email or other inferred identity.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeResponse {
    /// Gateway-local mapped user identifier.
    pub user_id: LifeWorkbenchUserId,
    /// Canonical opaque LifeOS user identifier.
    pub life_os_user_id: String,
    /// Current Gateway user status.
    pub status: String,
    /// Current Workbench Session identifier.
    pub session_id: WorkbenchSessionId,
    /// Deployment to which the session is bound.
    pub deployment_id: String,
    /// Current mirrored LifeOS memberships.
    pub memberships: Vec<ResolvedMembership>,
    /// Active and historical Nostr bindings.
    pub bindings: Vec<IdentityBinding>,
}

impl Store {
    /// Creates a hash-only Workbench Session after explicit LifeOS identity resolution.
    pub async fn create_workbench_session(
        &self,
        oidc: &OidcIdentity,
        resolved: &ResolvedLifeIdentity,
        deployment_id: &str,
        trace_id: Uuid,
    ) -> Result<IssuedSession, IdentityError> {
        validate_resolved(resolved)?;
        if !resolved.active {
            return Err(IdentityError::Inactive);
        }
        if !safe_text(deployment_id, 1, 256) || oidc.expires_at <= Utc::now() {
            return Err(IdentityError::Invalid);
        }
        let mut transaction = self.pool.begin().await.map_err(database)?;
        let existing = sqlx::query(
            "SELECT id,life_os_user_id,status FROM life_workbench_users
             WHERE oidc_issuer=$1 AND oidc_subject=$2 FOR UPDATE",
        )
        .bind(&oidc.issuer)
        .bind(&oidc.subject)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?;
        let user_id = if let Some(row) = existing {
            if row.get::<String, _>("status") != "active" {
                return Err(IdentityError::Inactive);
            }
            if row.get::<String, _>("life_os_user_id") != resolved.life_os_user_id {
                return Err(IdentityError::Conflict);
            }
            let id = row.get::<Uuid, _>("id");
            sqlx::query("UPDATE life_workbench_users SET updated_at=now() WHERE id=$1")
                .bind(id)
                .execute(&mut *transaction)
                .await
                .map_err(database)?;
            id
        } else {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO life_workbench_users
                 (id,oidc_issuer,oidc_subject,life_os_user_id,status)
                 VALUES($1,$2,$3,$4,'active')",
            )
            .bind(id)
            .bind(&oidc.issuer)
            .bind(&oidc.subject)
            .bind(&resolved.life_os_user_id)
            .execute(&mut *transaction)
            .await
            .map_err(database)?;
            id
        };
        sync_memberships(&mut transaction, user_id, &resolved.memberships).await?;

        let session_id = Uuid::new_v4();
        let session_token = random_token();
        sqlx::query(
            "INSERT INTO life_workbench_sessions
             (id,workbench_user_id,deployment_id,token_hash,status,expires_at)
             VALUES($1,$2,$3,$4,'active',$5)",
        )
        .bind(session_id)
        .bind(user_id)
        .bind(deployment_id)
        .bind(hash(&session_token))
        .bind(oidc.expires_at)
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        audit(
            &mut transaction,
            "WORKBENCH_SESSION_CREATED",
            "success",
            Some(user_id),
            Some(session_id),
            None,
            trace_id,
        )
        .await?;
        transaction.commit().await.map_err(database)?;
        Ok(IssuedSession {
            session_id: WorkbenchSessionId::new(session_id),
            session_token,
            expires_at: oidc.expires_at,
            user_id: LifeWorkbenchUserId::new(user_id),
            life_os_user_id: resolved.life_os_user_id.clone(),
        })
    }

    /// Resolves a presented session through its SHA-256 hash and fixed deployment.
    pub async fn authenticate_workbench_session(
        &self,
        session_token: &str,
        deployment_id: &str,
    ) -> Result<SessionPrincipal, IdentityError> {
        if session_token.is_empty() || session_token.len() > 512 {
            return Err(IdentityError::Unauthorized);
        }
        let row = sqlx::query(
            "SELECT s.id,s.workbench_user_id,s.deployment_id,
                    u.oidc_issuer,u.oidc_subject,u.life_os_user_id
             FROM life_workbench_sessions s
             JOIN life_workbench_users u ON u.id=s.workbench_user_id
             WHERE s.token_hash=$1 AND s.deployment_id=$2
               AND s.status='active' AND s.expires_at>now() AND u.status='active'",
        )
        .bind(hash(session_token))
        .bind(deployment_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(database)?
        .ok_or(IdentityError::Unauthorized)?;
        Ok(SessionPrincipal {
            session_id: WorkbenchSessionId::new(row.get("id")),
            user_id: LifeWorkbenchUserId::new(row.get("workbench_user_id")),
            issuer: row.get("oidc_issuer"),
            subject: row.get("oidc_subject"),
            life_os_user_id: row.get("life_os_user_id"),
            deployment_id: row.get("deployment_id"),
        })
    }

    /// Issues a short-lived challenge bound to one user, session, deployment, and pubkey.
    pub async fn create_identity_binding_challenge(
        &self,
        principal: &SessionPrincipal,
        pubkey: &str,
        ttl: Duration,
        trace_id: Uuid,
    ) -> Result<BindingChallenge, IdentityError> {
        if !valid_pubkey(pubkey) || ttl < Duration::seconds(30) || ttl > Duration::minutes(5) {
            return Err(IdentityError::Invalid);
        }
        let challenge_id = Uuid::new_v4();
        let nonce = random_token();
        let issued_at = Utc::now();
        let expires_at = issued_at + ttl;
        let payload = canonical_binding_payload(
            challenge_id,
            &nonce,
            principal,
            pubkey,
            issued_at.timestamp(),
            expires_at.timestamp(),
        );
        let mut transaction = self.pool.begin().await.map_err(database)?;
        sqlx::query(
            "UPDATE life_identity_binding_challenges
             SET status='revoked',revoked_at=now()
             WHERE workbench_user_id=$1 AND buzz_pubkey=$2 AND status='active'",
        )
        .bind(principal.user_id.as_uuid())
        .bind(pubkey)
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        sqlx::query(
            "INSERT INTO life_identity_binding_challenges
             (id,workbench_user_id,workbench_session_id,deployment_id,buzz_pubkey,
              nonce_hash,status,created_at,expires_at)
             VALUES($1,$2,$3,$4,$5,$6,'active',$7,$8)",
        )
        .bind(challenge_id)
        .bind(principal.user_id.as_uuid())
        .bind(principal.session_id.as_uuid())
        .bind(&principal.deployment_id)
        .bind(pubkey)
        .bind(hash(&nonce))
        .bind(issued_at)
        .bind(expires_at)
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        audit(
            &mut transaction,
            "IDENTITY_BINDING_CHALLENGE_CREATED",
            "success",
            Some(principal.user_id.as_uuid()),
            Some(principal.session_id.as_uuid()),
            None,
            trace_id,
        )
        .await?;
        transaction.commit().await.map_err(database)?;
        Ok(BindingChallenge {
            challenge_id: IdentityBindingChallengeId::new(challenge_id),
            audience: BINDING_AUDIENCE,
            canonical_payload: payload,
            expires_at,
            trace_id,
        })
    }

    /// Verifies and atomically consumes a complete signed kind `24243` event.
    pub async fn verify_identity_binding(
        &self,
        principal: &SessionPrincipal,
        challenge_id: IdentityBindingChallengeId,
        event: Event,
        trace_id: Uuid,
    ) -> Result<IdentityBinding, IdentityError> {
        let mut transaction = self.pool.begin().await.map_err(database)?;
        let row = sqlx::query(
            "SELECT c.buzz_pubkey,c.nonce_hash,c.status,c.created_at,c.expires_at,
                    c.workbench_session_id,c.deployment_id
             FROM life_identity_binding_challenges c
             WHERE c.id=$1 AND c.workbench_user_id=$2 FOR UPDATE",
        )
        .bind(challenge_id.as_uuid())
        .bind(principal.user_id.as_uuid())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?
        .ok_or(IdentityError::NotFound)?;
        let pubkey = row.get::<String, _>("buzz_pubkey");
        let status = row.get::<String, _>("status");
        let created_at = row.get::<DateTime<Utc>, _>("created_at");
        let expires_at = row.get::<DateTime<Utc>, _>("expires_at");
        let nonce = payload_value(&event.content, "nonce");
        let expected_payload = nonce.map(|nonce| {
            canonical_binding_payload(
                challenge_id.as_uuid(),
                nonce,
                principal,
                &pubkey,
                created_at.timestamp(),
                expires_at.timestamp(),
            )
        });
        let event_time = i64::try_from(event.created_at.as_secs()).ok();
        let valid = status == "active"
            && expires_at > Utc::now()
            && row.get::<Uuid, _>("workbench_session_id") == principal.session_id.as_uuid()
            && row.get::<String, _>("deployment_id") == principal.deployment_id
            && event.kind == Kind::Custom(24243)
            && event.pubkey.to_hex() == pubkey
            && expected_payload.as_deref() == Some(event.content.as_str())
            && nonce.is_some_and(|value| hash(value) == row.get::<Vec<u8>, _>("nonce_hash"))
            && event_time.is_some_and(|timestamp| {
                timestamp >= created_at.timestamp() && timestamp <= expires_at.timestamp()
            })
            && event.verify_id()
            && event.verify_signature();
        if !valid {
            if status == "active" && expires_at <= Utc::now() {
                sqlx::query(
                    "UPDATE life_identity_binding_challenges SET status='expired'
                     WHERE id=$1 AND status='active'",
                )
                .bind(challenge_id.as_uuid())
                .execute(&mut *transaction)
                .await
                .map_err(database)?;
            }
            audit(
                &mut transaction,
                "IDENTITY_BINDING_FAILED",
                "failure",
                Some(principal.user_id.as_uuid()),
                Some(principal.session_id.as_uuid()),
                None,
                trace_id,
            )
            .await?;
            transaction.commit().await.map_err(database)?;
            return Err(IdentityError::Invalid);
        }
        let consumed = sqlx::query_scalar::<_, Uuid>(
            "UPDATE life_identity_binding_challenges
             SET status='consumed',consumed_at=now()
             WHERE id=$1 AND status='active' AND expires_at>now()
             RETURNING id",
        )
        .bind(challenge_id.as_uuid())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?;
        if consumed.is_none() {
            return Err(IdentityError::Conflict);
        }

        if let Some(owner) = sqlx::query(
            "SELECT id,workbench_user_id FROM life_identity_bindings
             WHERE buzz_pubkey=$1 AND status='active' FOR UPDATE",
        )
        .bind(&pubkey)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?
        {
            let owner_user = owner.get::<Uuid, _>("workbench_user_id");
            if owner_user != principal.user_id.as_uuid() {
                audit(
                    &mut transaction,
                    "IDENTITY_BINDING_CONFLICT",
                    "denied",
                    Some(principal.user_id.as_uuid()),
                    Some(principal.session_id.as_uuid()),
                    None,
                    trace_id,
                )
                .await?;
                transaction.commit().await.map_err(database)?;
                return Err(IdentityError::Conflict);
            }
            let existing_id = owner.get::<Uuid, _>("id");
            let binding = binding_by_id(&mut transaction, existing_id).await?;
            audit(
                &mut transaction,
                "IDENTITY_BINDING_VERIFIED",
                "success",
                Some(principal.user_id.as_uuid()),
                Some(principal.session_id.as_uuid()),
                Some(existing_id),
                trace_id,
            )
            .await?;
            transaction.commit().await.map_err(database)?;
            return Ok(binding);
        }

        let binding_id = Uuid::new_v4();
        let binding = sqlx::query(
            "INSERT INTO life_identity_bindings
             (id,workbench_user_id,buzz_pubkey,source_event_id,status)
             VALUES($1,$2,$3,$4,'active')
             RETURNING id,buzz_pubkey,status,created_at,version",
        )
        .bind(binding_id)
        .bind(principal.user_id.as_uuid())
        .bind(&pubkey)
        .bind(event.id.to_hex())
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| {
            if error
                .as_database_error()
                .is_some_and(|db| db.is_unique_violation())
            {
                IdentityError::Conflict
            } else {
                IdentityError::Unavailable
            }
        })?;
        let binding = binding_from_row(&binding);
        audit(
            &mut transaction,
            "IDENTITY_BINDING_CREATED",
            "success",
            Some(principal.user_id.as_uuid()),
            Some(principal.session_id.as_uuid()),
            Some(binding_id),
            trace_id,
        )
        .await?;
        transaction.commit().await.map_err(database)?;
        Ok(binding)
    }

    /// Revokes a binding and all binding-scoped delegations and Embed Sessions atomically.
    pub async fn revoke_identity_binding(
        &self,
        principal: &SessionPrincipal,
        binding_id: IdentityBindingId,
        trace_id: Uuid,
    ) -> Result<(), IdentityError> {
        let mut transaction = self.pool.begin().await.map_err(database)?;
        let revoked = sqlx::query_scalar::<_, String>(
            "UPDATE life_identity_bindings
             SET status='revoked',revoked_at=now(),version=version+1
             WHERE id=$1 AND workbench_user_id=$2 AND status='active'
             RETURNING buzz_pubkey",
        )
        .bind(binding_id.as_uuid())
        .bind(principal.user_id.as_uuid())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?;
        if revoked.is_none() {
            return Err(IdentityError::NotFound);
        }
        sqlx::query(
            "UPDATE life_agent_delegations
             SET status='revoked',revoked_at=now()
             WHERE identity_binding_id=$1 AND status IN ('active','exhausted')",
        )
        .bind(binding_id.as_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        sqlx::query(
            "UPDATE life_embed_sessions
             SET status='revoked',revoked_at=now()
             WHERE identity_binding_id=$1 AND status='active'",
        )
        .bind(binding_id.as_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        audit(
            &mut transaction,
            "IDENTITY_BINDING_REVOKED",
            "success",
            Some(principal.user_id.as_uuid()),
            Some(principal.session_id.as_uuid()),
            Some(binding_id.as_uuid()),
            trace_id,
        )
        .await?;
        transaction.commit().await.map_err(database)?;
        Ok(())
    }

    /// Returns the mapped Life identity, current mirrored memberships, and binding history.
    pub async fn me(&self, principal: &SessionPrincipal) -> Result<MeResponse, IdentityError> {
        let status =
            sqlx::query_scalar::<_, String>("SELECT status FROM life_workbench_users WHERE id=$1")
                .bind(principal.user_id.as_uuid())
                .fetch_one(&self.pool)
                .await
                .map_err(database)?;
        let memberships = sqlx::query(
            "SELECT workspace_id,role_code,membership_version
             FROM life_workspace_memberships
             WHERE workbench_user_id=$1 AND status='active' ORDER BY workspace_id",
        )
        .bind(principal.user_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(database)?
        .into_iter()
        .map(|row| ResolvedMembership {
            workspace_id: row.get("workspace_id"),
            role: row.get("role_code"),
            membership_version: row.get("membership_version"),
        })
        .collect();
        let bindings = sqlx::query(
            "SELECT id,buzz_pubkey,status,created_at,version
             FROM life_identity_bindings WHERE workbench_user_id=$1 ORDER BY created_at DESC",
        )
        .bind(principal.user_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(database)?
        .iter()
        .map(binding_from_row)
        .collect();
        Ok(MeResponse {
            user_id: principal.user_id,
            life_os_user_id: principal.life_os_user_id.clone(),
            status,
            session_id: principal.session_id,
            deployment_id: principal.deployment_id.clone(),
            memberships,
            bindings,
        })
    }
}

fn validate_resolved(resolved: &ResolvedLifeIdentity) -> Result<(), IdentityError> {
    if !safe_text(&resolved.life_os_user_id, 1, 512)
        || resolved.memberships.iter().any(|membership| {
            !safe_text(&membership.workspace_id, 1, 512)
                || !matches!(
                    membership.role.as_str(),
                    "OWNER" | "ADMIN" | "MEMBER" | "VIEWER"
                )
                || membership.membership_version < 1
        })
    {
        return Err(IdentityError::Invalid);
    }
    Ok(())
}

async fn sync_memberships(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    memberships: &[ResolvedMembership],
) -> Result<(), IdentityError> {
    let workspace_ids = memberships
        .iter()
        .map(|membership| membership.workspace_id.clone())
        .collect::<Vec<_>>();
    sqlx::query(
        "UPDATE life_workspace_memberships
         SET status='revoked',revoked_at=now(),updated_at=now()
         WHERE workbench_user_id=$1 AND status='active'
           AND workspace_id <> ALL($2::text[])",
    )
    .bind(user_id)
    .bind(&workspace_ids)
    .execute(&mut **transaction)
    .await
    .map_err(database)?;
    for membership in memberships {
        let updated = sqlx::query(
            "UPDATE life_workspace_memberships
             SET role_code=$3,membership_version=$4,updated_at=now()
             WHERE workbench_user_id=$1 AND workspace_id=$2 AND status='active'",
        )
        .bind(user_id)
        .bind(&membership.workspace_id)
        .bind(&membership.role)
        .bind(membership.membership_version)
        .execute(&mut **transaction)
        .await
        .map_err(database)?;
        if updated.rows_affected() == 0 {
            sqlx::query(
                "INSERT INTO life_workspace_memberships
                 (id,workbench_user_id,workspace_id,role_code,status,membership_version)
                 VALUES($1,$2,$3,$4,'active',$5)",
            )
            .bind(Uuid::new_v4())
            .bind(user_id)
            .bind(&membership.workspace_id)
            .bind(&membership.role)
            .bind(membership.membership_version)
            .execute(&mut **transaction)
            .await
            .map_err(database)?;
        }
    }
    let authority_version = memberships
        .iter()
        .map(|membership| membership.membership_version)
        .max()
        .unwrap_or(0);
    sqlx::query(
        "UPDATE life_workbench_users
         SET authority_version=GREATEST(authority_version,$2),
             authority_sync_status='current',authority_synced_at=now(),updated_at=now()
         WHERE id=$1",
    )
    .bind(user_id)
    .bind(authority_version)
    .execute(&mut **transaction)
    .await
    .map_err(database)?;
    Ok(())
}

fn canonical_binding_payload(
    challenge_id: Uuid,
    nonce: &str,
    principal: &SessionPrincipal,
    pubkey: &str,
    issued_at: i64,
    expires_at: i64,
) -> String {
    format!(
        "life-workbench-identity-binding-v1\nchallenge_id={challenge_id}\nnonce={nonce}\naudience={BINDING_AUDIENCE}\noidc_issuer={}\noidc_subject={}\nlife_os_user_id={}\nworkbench_session_id={}\ndeployment_id={}\nbuzz_pubkey={pubkey}\nissued_at={issued_at}\nexpires_at={expires_at}",
        principal.issuer,
        principal.subject,
        principal.life_os_user_id,
        principal.session_id.as_uuid(),
        principal.deployment_id,
    )
}

fn payload_value<'a>(payload: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("{name}=");
    let mut matches = payload
        .lines()
        .filter_map(|line| line.strip_prefix(&prefix));
    let value = matches.next()?;
    if matches.next().is_some() || value.is_empty() || value.len() > 512 {
        return None;
    }
    Some(value)
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn hash(value: &str) -> Vec<u8> {
    Sha256::digest(value.as_bytes()).to_vec()
}

fn valid_pubkey(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn safe_text(value: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&value.len())
        && value.trim() == value
        && !value.contains(['\r', '\n', '\0'])
}

async fn binding_by_id(
    transaction: &mut Transaction<'_, Postgres>,
    binding_id: Uuid,
) -> Result<IdentityBinding, IdentityError> {
    let row = sqlx::query(
        "SELECT id,buzz_pubkey,status,created_at,version
         FROM life_identity_bindings WHERE id=$1",
    )
    .bind(binding_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(database)?;
    Ok(binding_from_row(&row))
}

fn binding_from_row(row: &sqlx::postgres::PgRow) -> IdentityBinding {
    IdentityBinding {
        binding_id: IdentityBindingId::new(row.get("id")),
        pubkey: row.get("buzz_pubkey"),
        status: row.get("status"),
        created_at: row.get("created_at"),
        version: row.get("version"),
    }
}

async fn audit(
    transaction: &mut Transaction<'_, Postgres>,
    event_type: &str,
    outcome: &str,
    user_id: Option<Uuid>,
    session_id: Option<Uuid>,
    binding_id: Option<Uuid>,
    trace_id: Uuid,
) -> Result<(), IdentityError> {
    sqlx::query(
        "INSERT INTO life_security_audit
         (event_type,outcome,subject_kind,subject_id,workbench_user_id,
          workbench_session_id,identity_binding_id,trace_id)
         VALUES($1,$2,'human',$3,$4,$5,$6,$7)",
    )
    .bind(event_type)
    .bind(outcome)
    .bind(user_id.map(|id| id.to_string()))
    .bind(user_id)
    .bind(session_id)
    .bind(binding_id)
    .bind(trace_id)
    .execute(&mut **transaction)
    .await
    .map_err(database)?;
    Ok(())
}

fn database(_: sqlx::Error) -> IdentityError {
    IdentityError::Unavailable
}
