use crate::model::{AgentDelegationId, IdentityBindingChallengeId, LifeWorkbenchUserId};
use sqlx::{PgPool, Postgres, Transaction};

/// Stable failure classes returned by the Life security store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// PostgreSQL was unavailable or rejected the requested transition.
    #[error("life security store unavailable")]
    Database,
}

/// Transactional persistence boundary for Life identity and delegation state.
#[derive(Clone)]
pub struct Store {
    pool: PgPool,
}

impl Store {
    /// Creates a store over the isolated Life authentication database pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Applies the forward-only Life authentication migration set.
    pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!().run(pool).await
    }

    /// Verifies that the isolated PostgreSQL dependency is accepting queries.
    pub async fn ready(&self) -> Result<(), StoreError> {
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map(|_| ())
            .map_err(|_| StoreError::Database)
    }

    /// Atomically consumes one active, unexpired identity-binding challenge.
    pub async fn consume_identity_binding_challenge(
        &self,
        challenge_id: IdentityBindingChallengeId,
        user_id: LifeWorkbenchUserId,
    ) -> Result<bool, StoreError> {
        let mut transaction = self.transaction().await?;
        let consumed = sqlx::query_scalar::<_, uuid::Uuid>(
            "UPDATE life_identity_binding_challenges
             SET status='consumed', consumed_at=now()
             WHERE id=$1 AND workbench_user_id=$2
               AND status='active' AND expires_at>now()
             RETURNING id",
        )
        .bind(challenge_id.as_uuid())
        .bind(user_id.as_uuid())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| StoreError::Database)?
        .is_some();
        transaction
            .commit()
            .await
            .map_err(|_| StoreError::Database)?;
        Ok(consumed)
    }

    /// Atomically revokes one currently active Agent delegation.
    pub async fn revoke_agent_delegation(
        &self,
        delegation_id: AgentDelegationId,
    ) -> Result<bool, StoreError> {
        let mut transaction = self.transaction().await?;
        let revoked = sqlx::query_scalar::<_, uuid::Uuid>(
            "UPDATE life_agent_delegations
             SET status='revoked', revoked_at=now()
             WHERE id=$1 AND status='active'
             RETURNING id",
        )
        .bind(delegation_id.as_uuid())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| StoreError::Database)?
        .is_some();
        transaction
            .commit()
            .await
            .map_err(|_| StoreError::Database)?;
        Ok(revoked)
    }

    async fn transaction(&self) -> Result<Transaction<'_, Postgres>, StoreError> {
        self.pool.begin().await.map_err(|_| StoreError::Database)
    }
}
