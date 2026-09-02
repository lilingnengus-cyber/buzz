//! One-time Life Dock bootstrap codes and revocable hash-only Embed Sessions.

use crate::{
    identity::SessionPrincipal,
    model::{EmbedSessionId, LifeWorkbenchUserId, WorkbenchSessionId},
    Store,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration as ChronoDuration, NaiveDate, Utc};
use rand::RngExt as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::time::Duration;
use uuid::Uuid;

/// Fixed lifetimes for a one-time code and the resulting Dock session.
#[derive(Clone, Debug)]
pub struct EmbedPolicy {
    code_ttl: Duration,
    session_ttl: Duration,
}

impl EmbedPolicy {
    /// Validates a code TTL of 5–120 seconds and session TTL of 1 minute–24 hours.
    pub fn new(code_ttl: Duration, session_ttl: Duration) -> Result<Self, EmbedError> {
        if !(5..=120).contains(&code_ttl.as_secs())
            || !(60..=86_400).contains(&session_ttl.as_secs())
        {
            return Err(EmbedError::Invalid);
        }
        Ok(Self {
            code_ttl,
            session_ttl,
        })
    }

    pub(crate) fn standard() -> Self {
        Self {
            code_ttl: Duration::from_secs(30),
            session_ttl: Duration::from_secs(86_400),
        }
    }
}

/// Request to open one allowlisted Life Dock target.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IssueEmbedRequest {
    /// Exact local `/embed/...` path; origins and arbitrary queries are rejected.
    pub target_path: String,
}

/// Hash-only network facts retained for security correlation without raw IP or User-Agent data.
#[derive(Clone, Debug, Default)]
pub struct EmbedRiskFacts {
    ip_hash: Option<Vec<u8>>,
    user_agent_hash: Option<Vec<u8>>,
}

impl EmbedRiskFacts {
    /// Hashes bounded request facts before they reach persistence.
    pub fn from_request(ip: Option<&str>, user_agent: Option<&str>) -> Self {
        Self {
            ip_hash: bounded_fact(ip, 256).map(hash),
            user_agent_hash: bounded_fact(user_agent, 1024).map(hash),
        }
    }
}

/// One-time plaintext bootstrap code returned only to the authenticated Workbench.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuedEmbedCode {
    /// Stable lifecycle identifier used for revocation before or after consumption.
    pub embed_session_id: EmbedSessionId,
    /// Random 32-byte Base64URL code; it must never be logged.
    pub code: String,
    /// Code expiration instant.
    pub expires_at: chrono::DateTime<Utc>,
    /// Distributed trace identifier.
    pub trace_id: Uuid,
}

/// LifeOS service request to atomically redeem a bootstrap code.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumeEmbedRequest {
    /// One-time 32-byte Base64URL code.
    pub code: String,
}

/// Hash-only Dock session material returned once to the trusted LifeOS service.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumedEmbedSession {
    /// Stable Embed Session identifier.
    pub embed_session_id: EmbedSessionId,
    /// Random 32-byte session bearer; LifeOS must place it in an HttpOnly cookie.
    pub session_token: String,
    /// Bound Life Workbench user.
    pub workbench_user_id: LifeWorkbenchUserId,
    /// Canonical LifeOS user resolved when the Workbench Session was created.
    pub life_os_user_id: String,
    /// Bound Workbench Session.
    pub workbench_session_id: WorkbenchSessionId,
    /// Bound Pacioli deployment.
    pub deployment_id: String,
    /// Exact allowlisted redirect path.
    pub target_path: String,
    /// Dock session expiration, never later than the parent Workbench Session.
    pub expires_at: chrono::DateTime<Utc>,
    /// Distributed trace identifier inherited from code issuance.
    pub trace_id: Uuid,
}

/// Stable fail-closed Embed Session failures.
#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    /// TTL, code, deployment, or target path was invalid.
    #[error("Life Embed request is invalid")]
    Invalid,
    /// Code or current bound authority was rejected.
    #[error("Life Embed request is unauthorized")]
    Unauthorized,
    /// Owned lifecycle record was not active.
    #[error("Life Embed session was not found")]
    NotFound,
    /// PostgreSQL could not complete the atomic transition.
    #[error("Life Embed store unavailable")]
    Database,
}

impl Store {
    /// Issues a hash-only, short-lived code bound to current user/session/deployment state.
    pub async fn issue_embed_code(
        &self,
        principal: &SessionPrincipal,
        request: IssueEmbedRequest,
        policy: &EmbedPolicy,
        risk_facts: &EmbedRiskFacts,
        trace_id: Uuid,
    ) -> Result<IssuedEmbedCode, EmbedError> {
        if !allowlisted_target(&request.target_path) {
            return Err(EmbedError::Invalid);
        }
        let mut transaction = self.pool.begin().await.map_err(database)?;
        let current = sqlx::query_scalar::<_, bool>(
            "SELECT true FROM life_workbench_sessions s
             JOIN life_workbench_users u ON u.id=s.workbench_user_id
             WHERE s.id=$1 AND s.workbench_user_id=$2 AND s.deployment_id=$3
               AND s.status='active' AND s.expires_at>now() AND u.status='active'
             FOR UPDATE OF s,u",
        )
        .bind(principal.session_id.as_uuid())
        .bind(principal.user_id.as_uuid())
        .bind(&principal.deployment_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?
        .is_some();
        if !current {
            return Err(EmbedError::Unauthorized);
        }
        let id = Uuid::new_v4();
        let code = random_token();
        let expires_at = Utc::now()
            + ChronoDuration::from_std(policy.code_ttl).map_err(|_| EmbedError::Invalid)?;
        sqlx::query(
            "INSERT INTO life_embed_codes
             (id,code_hash,workbench_user_id,workbench_session_id,deployment_id,
              target_path,status,expires_at,trace_id,issue_ip_hash,issue_user_agent_hash)
             VALUES($1,$2,$3,$4,$5,$6,'active',$7,$8,$9,$10)",
        )
        .bind(id)
        .bind(hash(&code))
        .bind(principal.user_id.as_uuid())
        .bind(principal.session_id.as_uuid())
        .bind(&principal.deployment_id)
        .bind(&request.target_path)
        .bind(expires_at)
        .bind(trace_id)
        .bind(&risk_facts.ip_hash)
        .bind(&risk_facts.user_agent_hash)
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        audit(
            &mut transaction,
            "EMBED_SESSION_ISSUED",
            id,
            principal.user_id.as_uuid(),
            principal.session_id.as_uuid(),
            trace_id,
        )
        .await?;
        transaction.commit().await.map_err(database)?;
        Ok(IssuedEmbedCode {
            embed_session_id: EmbedSessionId::new(id),
            code,
            expires_at,
            trace_id,
        })
    }

    /// Atomically burns one code and creates exactly one hash-only Dock session.
    pub async fn consume_embed_code(
        &self,
        code: &str,
        deployment_id: &str,
        policy: &EmbedPolicy,
        risk_facts: &EmbedRiskFacts,
        request_trace_id: Uuid,
    ) -> Result<ConsumedEmbedSession, EmbedError> {
        validate_token(code)?;
        if !safe_identifier(deployment_id, 256) {
            return Err(EmbedError::Invalid);
        }
        let code_hash = hash(code);
        let mut transaction = self.pool.begin().await.map_err(database)?;
        sqlx::query(
            "UPDATE life_embed_codes SET status='expired'
             WHERE code_hash=$1 AND status='active' AND expires_at<=now()",
        )
        .bind(&code_hash)
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        let row = sqlx::query(
            "UPDATE life_embed_codes c
             SET status='consumed',consumed_at=now()
             FROM life_workbench_users u,life_workbench_sessions s
             WHERE c.code_hash=$1 AND c.workbench_user_id=u.id
               AND c.workbench_session_id=s.id AND c.status='active'
               AND c.expires_at>now() AND c.deployment_id=$2
               AND u.status='active' AND s.status='active' AND s.expires_at>now()
               AND s.workbench_user_id=c.workbench_user_id
               AND s.deployment_id=c.deployment_id
             RETURNING c.id,c.workbench_user_id,c.workbench_session_id,c.deployment_id,
                       c.target_path,c.trace_id,s.expires_at AS workbench_expires_at,
                       u.life_os_user_id",
        )
        .bind(&code_hash)
        .bind(deployment_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?;
        let Some(row) = row else {
            transaction.commit().await.map_err(database)?;
            return Err(EmbedError::Unauthorized);
        };
        let id: Uuid = row.get("id");
        let user_id: Uuid = row.get("workbench_user_id");
        let workbench_session_id: Uuid = row.get("workbench_session_id");
        let trace_id: Uuid = row.get("trace_id");
        let workbench_expires_at = row.get::<chrono::DateTime<Utc>, _>("workbench_expires_at");
        let session_limit = Utc::now()
            + ChronoDuration::from_std(policy.session_ttl).map_err(|_| EmbedError::Invalid)?;
        let expires_at = session_limit.min(workbench_expires_at);
        let session_token = random_token();
        sqlx::query(
            "INSERT INTO life_embed_sessions
             (id,session_token_hash,workbench_user_id,workbench_session_id,deployment_id,
              status,expires_at,trace_id,consume_ip_hash,consume_user_agent_hash)
             VALUES($1,$2,$3,$4,$5,'active',$6,$7,$8,$9)",
        )
        .bind(id)
        .bind(hash(&session_token))
        .bind(user_id)
        .bind(workbench_session_id)
        .bind(deployment_id)
        .bind(expires_at)
        .bind(trace_id)
        .bind(&risk_facts.ip_hash)
        .bind(&risk_facts.user_agent_hash)
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        audit(
            &mut transaction,
            "EMBED_SESSION_CONSUMED",
            id,
            user_id,
            workbench_session_id,
            trace_id,
        )
        .await?;
        transaction.commit().await.map_err(database)?;
        tracing::debug!(trace_id = %request_trace_id, embed_session_id = %id, "Life Embed code consumed");
        Ok(ConsumedEmbedSession {
            embed_session_id: EmbedSessionId::new(id),
            session_token,
            workbench_user_id: LifeWorkbenchUserId::new(user_id),
            life_os_user_id: row.get("life_os_user_id"),
            workbench_session_id: WorkbenchSessionId::new(workbench_session_id),
            deployment_id: row.get("deployment_id"),
            target_path: row.get("target_path"),
            expires_at,
            trace_id,
        })
    }

    /// Revokes an owned pending code or active Dock session under its parent Workbench Session.
    pub async fn revoke_embed_session(
        &self,
        principal: &SessionPrincipal,
        id: EmbedSessionId,
        trace_id: Uuid,
    ) -> Result<(), EmbedError> {
        let mut transaction = self.pool.begin().await.map_err(database)?;
        let code = sqlx::query(
            "UPDATE life_embed_codes SET status='revoked',revoked_at=now()
             WHERE id=$1 AND workbench_user_id=$2 AND workbench_session_id=$3
               AND deployment_id=$4 AND status='active'",
        )
        .bind(id.as_uuid())
        .bind(principal.user_id.as_uuid())
        .bind(principal.session_id.as_uuid())
        .bind(&principal.deployment_id)
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        let session = sqlx::query(
            "UPDATE life_embed_sessions SET status='revoked',revoked_at=now()
             WHERE id=$1 AND workbench_user_id=$2 AND workbench_session_id=$3
               AND deployment_id=$4 AND status='active'",
        )
        .bind(id.as_uuid())
        .bind(principal.user_id.as_uuid())
        .bind(principal.session_id.as_uuid())
        .bind(&principal.deployment_id)
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        if code.rows_affected() + session.rows_affected() == 0 {
            return Err(EmbedError::NotFound);
        }
        audit(
            &mut transaction,
            "EMBED_SESSION_REVOKED",
            id.as_uuid(),
            principal.user_id.as_uuid(),
            principal.session_id.as_uuid(),
            trace_id,
        )
        .await?;
        transaction.commit().await.map_err(database)?;
        Ok(())
    }
}

fn allowlisted_target(value: &str) -> bool {
    if value == "/embed/dashboard" {
        return true;
    }
    if let Some(date) = value.strip_prefix("/embed/calendar?date=") {
        return date.len() == 10
            && NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .is_ok_and(|parsed| parsed.format("%Y-%m-%d").to_string() == date);
    }
    if value.contains(['?', '#', '%']) {
        return false;
    }
    const PREFIXES: &[&str] = &[
        "/embed/domains/",
        "/embed/goals/",
        "/embed/projects/",
        "/embed/actions/",
        "/embed/journal/",
        "/embed/knowledge/",
        "/embed/reviews/",
        "/embed/ai-executions/",
        "/embed/drafts/",
    ];
    PREFIXES.iter().any(|prefix| {
        value.strip_prefix(prefix).is_some_and(|id| {
            (1..=128).contains(&id.len())
                && id != "."
                && id != ".."
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-._~".contains(&byte))
        })
    })
}

fn validate_token(token: &str) -> Result<(), EmbedError> {
    if URL_SAFE_NO_PAD
        .decode(token)
        .ok()
        .is_none_or(|bytes| bytes.len() != 32)
    {
        return Err(EmbedError::Invalid);
    }
    Ok(())
}

fn safe_identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn hash(value: &str) -> Vec<u8> {
    Sha256::digest(value.as_bytes()).to_vec()
}

fn bounded_fact(value: Option<&str>, max: usize) -> Option<&str> {
    value.filter(|value| {
        !value.is_empty()
            && value.len() <= max
            && !value.chars().any(|character| character.is_control())
    })
}

async fn audit(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    event_type: &str,
    id: Uuid,
    user_id: Uuid,
    workbench_session_id: Uuid,
    trace_id: Uuid,
) -> Result<(), EmbedError> {
    sqlx::query(
        "INSERT INTO life_security_audit
         (event_type,outcome,subject_kind,subject_id,workbench_user_id,
          workbench_session_id,trace_id)
         VALUES($1,'success','embed_session',$2,$3,$4,$5)",
    )
    .bind(event_type)
    .bind(id.to_string())
    .bind(user_id)
    .bind(workbench_session_id)
    .bind(trace_id)
    .execute(&mut **transaction)
    .await
    .map_err(database)?;
    Ok(())
}

fn database(_: sqlx::Error) -> EmbedError {
    EmbedError::Database
}
