//! Recheck authority after resource waits, before any CRM write can commit.
use super::{CrmService, Opportunity};
use crate::{b2::common::DomainError, store::PgStore};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

impl CrmService {
    pub(super) async fn check_write_authority(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        id: Uuid,
    ) -> Result<(), DomainError> {
        // Normal writes already hold the row lock; replay must also stabilize
        // the current customer before validating access to the saved result.
        // Use the same exclusive lock for pre-update checks to avoid upgrades.
        let current = sqlx::query_as::<_, Opportunity>(
            "SELECT * FROM crm_opportunities WHERE id=$1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
        sqlx::query(
            "SELECT revision FROM business_authorization_revision WHERE singleton FOR SHARE",
        )
        .fetch_one(&mut **tx)
        .await?;
        let scope = PgStore::snapshot_on(&mut *tx, actor)
            .await
            .map_err(|_| DomainError::NotFoundOrForbidden)?;
        if !scope.permission_keys.contains("crm:manage")
            || !scope
                .scopes
                .legal_entity_ids
                .contains(&current.legal_entity_id)
            || !scope
                .scopes
                .business_unit_ids
                .contains(&current.business_unit_id)
            || current
                .customer_id
                .is_some_and(|customer| !scope.scopes.customer_ids.contains(&customer))
        {
            return Err(DomainError::NotFoundOrForbidden);
        }
        Ok(())
    }
}
