use crate::{b2::DomainError, security::valid_key, store::PgStore};
use uuid::Uuid;

/// One active operating-unit preference visible to its owning user.
pub struct OperatingUnitPreference {
    /// The selected active operating unit.
    pub business_unit_id: Uuid,
    /// Whether automatic recent-use updates must preserve this selection.
    pub pinned: bool,
}

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
        Ok(self
            .operating_unit_preference(actor, context)
            .await?
            .map(|preference| preference.business_unit_id))
    }

    /// Returns the active preference together with its fixed/default mode.
    pub async fn operating_unit_preference(
        &self,
        actor: Uuid,
        context: &str,
    ) -> Result<Option<OperatingUnitPreference>, DomainError> {
        validate_context(context)?;
        let snapshot = self
            .store
            .snapshot(actor)
            .await
            .map_err(|_| DomainError::NotFoundOrForbidden)?;
        let preference = sqlx::query_as::<_, (Uuid, bool)>(
            "SELECT preference.business_unit_id,preference.is_pinned
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
        Ok(preference
            .filter(|(id, _)| snapshot.scopes.business_unit_ids.contains(id))
            .map(|(business_unit_id, pinned)| OperatingUnitPreference {
                business_unit_id,
                pinned,
            }))
    }

    /// Resolves an explicit unit, an account preference, or the sole active unit.
    pub async fn resolve_operating_unit(
        &self,
        actor: Uuid,
        context: &str,
        explicit: Option<Uuid>,
    ) -> Result<Uuid, DomainError> {
        validate_context(context)?;
        let snapshot = self
            .store
            .snapshot(actor)
            .await
            .map_err(|_| DomainError::NotFoundOrForbidden)?;
        if let Some(id) = explicit {
            if snapshot.scopes.business_unit_ids.contains(&id) {
                let active = sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(SELECT 1 FROM business_units WHERE id=$1 AND status='active')",
                )
                .bind(id)
                .fetch_one(self.store.pool())
                .await?;
                if active {
                    return Ok(id);
                }
            }
            return Err(DomainError::NotFoundOrForbidden);
        }
        if let Some(preference) = self.operating_unit(actor, context).await? {
            return Ok(preference);
        }
        let candidates = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM business_units
             WHERE status='active' AND id=ANY($1)
             ORDER BY id LIMIT 2",
        )
        .bind(
            snapshot
                .scopes
                .business_unit_ids
                .iter()
                .copied()
                .collect::<Vec<_>>(),
        )
        .fetch_all(self.store.pool())
        .await?;
        match candidates.as_slice() {
            [id] => Ok(*id),
            [] => Err(DomainError::NotFoundOrForbidden),
            _ => Err(DomainError::Invalid(
                "businessUnitId is required when no unique account default is available".into(),
            )),
        }
    }

    /// Saves an operating-unit preference when the actor can access the unit.
    pub async fn save_operating_unit(
        &self,
        actor: Uuid,
        context: &str,
        business_unit_id: Uuid,
        pinned: bool,
    ) -> Result<OperatingUnitPreference, DomainError> {
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
        let preference = sqlx::query_as::<_, (Uuid, bool)>(
            "INSERT INTO business_user_operating_unit_preferences(
                enterprise_user_id,context,business_unit_id,is_pinned
             ) VALUES($1,$2,$3,$4)
             ON CONFLICT(enterprise_user_id,context) DO UPDATE SET
                business_unit_id=CASE
                    WHEN business_user_operating_unit_preferences.is_pinned
                         AND NOT EXCLUDED.is_pinned
                    THEN business_user_operating_unit_preferences.business_unit_id
                    ELSE EXCLUDED.business_unit_id
                END,
                is_pinned=business_user_operating_unit_preferences.is_pinned
                          OR EXCLUDED.is_pinned,
                updated_at=now()
             RETURNING business_unit_id,is_pinned",
        )
        .bind(actor)
        .bind(context)
        .bind(business_unit_id)
        .bind(pinned)
        .fetch_one(self.store.pool())
        .await?;
        Ok(OperatingUnitPreference {
            business_unit_id: preference.0,
            pinned: preference.1,
        })
    }

    /// Clears one workflow's operating-unit preference for the actor.
    pub async fn clear_operating_unit(
        &self,
        actor: Uuid,
        context: &str,
    ) -> Result<(), DomainError> {
        validate_context(context)?;
        self.store
            .snapshot(actor)
            .await
            .map_err(|_| DomainError::NotFoundOrForbidden)?;
        sqlx::query(
            "DELETE FROM business_user_operating_unit_preferences
             WHERE enterprise_user_id=$1 AND context=$2",
        )
        .bind(actor)
        .bind(context)
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
