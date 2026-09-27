use crate::{b2::DomainError, security::valid_key, store::PgStore};
use uuid::Uuid;

#[derive(Clone)]
/// Persists per-user Business workspace defaults after validating current scope.
pub struct UserPreferenceService {
    store: PgStore,
}

impl UserPreferenceService {
    /// Creates a preference service backed by the Business Core store.
    pub fn new(store: PgStore) -> Self {
        Self { store }
    }

    /// Returns the active, currently accessible operating-unit preference.
    pub async fn operating_unit(
        &self,
        actor: Uuid,
        context: &str,
    ) -> Result<Option<Uuid>, DomainError> {
        validate_context(context)?;
        let snapshot = self
            .store
            .snapshot(actor)
            .await
            .map_err(|_| DomainError::NotFoundOrForbidden)?;
        let unit = sqlx::query_scalar::<_, Uuid>(
            "SELECT preference.business_unit_id
             FROM business_user_operating_unit_preferences preference
             JOIN business_units unit ON unit.id=preference.business_unit_id
             WHERE preference.enterprise_user_id=$1
               AND preference.context=$2
               AND unit.status='active'",
        )
        .bind(actor)
        .bind(context)
        .fetch_optional(self.store.pool())
        .await?;
        Ok(unit.filter(|id| snapshot.scopes.business_unit_ids.contains(id)))
    }

    /// Saves an operating-unit preference when the actor can access the unit.
    pub async fn save_operating_unit(
        &self,
        actor: Uuid,
        context: &str,
        business_unit_id: Uuid,
    ) -> Result<(), DomainError> {
        validate_context(context)?;
        let snapshot = self
            .store
            .snapshot(actor)
            .await
            .map_err(|_| DomainError::NotFoundOrForbidden)?;
        if !snapshot
            .scopes
            .business_unit_ids
            .contains(&business_unit_id)
        {
            return Err(DomainError::NotFoundOrForbidden);
        }
        let active = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1 FROM business_units
                WHERE id=$1 AND status='active'
            )",
        )
        .bind(business_unit_id)
        .fetch_one(self.store.pool())
        .await?;
        if !active {
            return Err(DomainError::NotFoundOrForbidden);
        }
        sqlx::query(
            "INSERT INTO business_user_operating_unit_preferences(
                enterprise_user_id,context,business_unit_id
             ) VALUES($1,$2,$3)
             ON CONFLICT(enterprise_user_id,context) DO UPDATE SET
                business_unit_id=EXCLUDED.business_unit_id,
                updated_at=now()",
        )
        .bind(actor)
        .bind(context)
        .bind(business_unit_id)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }
}

fn validate_context(context: &str) -> Result<(), DomainError> {
    if valid_key(context, 64) {
        Ok(())
    } else {
        Err(DomainError::Invalid("invalid preference context".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::validate_context;

    #[test]
    fn preference_context_is_closed_form() {
        assert!(validate_context("sales-order").is_ok());
        assert!(validate_context("core-master-customer").is_ok());
        assert!(validate_context("Sales Order").is_err());
    }
}
