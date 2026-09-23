use super::*;
use crate::model::{AuthorizationSnapshot, DataScopes};
pub(super) async fn resolve(
    tx: &mut Transaction<'_, Postgres>,
    auth: &AuthorizationSnapshot,
    input: &GenerateOperatingSnapshot,
) -> Result<(DataScopes, String), DomainError> {
    let mut scopes = auth.scopes.clone();
    if input.legal_entity_ids.is_none()
        && input.business_unit_ids.is_none()
        && input.warehouse_ids.is_none()
    {
        return Ok((scopes, auth.effective_scope_hash.clone()));
    }
    if let Some(ids) = &input.legal_entity_ids {
        if ids.is_empty() {
            return Err(DomainError::Invalid(
                "legalEntityIds must not be empty".into(),
            ));
        }
        if ids.iter().any(|id| !scopes.legal_entity_ids.contains(id)) {
            return Err(DomainError::NotFoundOrForbidden);
        }
        scopes.legal_entity_ids = ids.iter().copied().collect();
    }
    // Warehouse ownership remains a legal boundary. The other scopes are
    // independent dimensions and must not be narrowed by the selected legal entities.
    let allowed_warehouses = scopes.warehouse_ids.iter().copied().collect::<Vec<_>>();
    let legal = scopes.legal_entity_ids.iter().copied().collect::<Vec<_>>();
    scopes.warehouse_ids = sqlx::query_scalar(
        "SELECT id FROM business_warehouses WHERE id=ANY($1) AND legal_entity_id=ANY($2) ORDER BY id",
    )
    .bind(allowed_warehouses)
    .bind(legal)
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .collect();
    if let Some(ids) = &input.business_unit_ids {
        if ids.is_empty() {
            return Err(DomainError::Invalid(
                "businessUnitIds must not be empty".into(),
            ));
        }
        if ids.iter().any(|id| !scopes.business_unit_ids.contains(id)) {
            return Err(DomainError::NotFoundOrForbidden);
        }
        let roots = ids.iter().copied().collect();
        let descendants = crate::operating_units::descendant_ids(&mut **tx, &roots, true).await?;
        scopes.business_unit_ids = descendants
            .intersection(&auth.scopes.business_unit_ids)
            .copied()
            .collect();
    }
    if let Some(ids) = &input.warehouse_ids {
        if ids.is_empty() {
            return Err(DomainError::Invalid(
                "warehouseIds must not be empty".into(),
            ));
        }
        if ids.iter().any(|id| !scopes.warehouse_ids.contains(id)) {
            return Err(DomainError::NotFoundOrForbidden);
        }
        scopes.warehouse_ids = ids.iter().copied().collect();
    }
    let version = if input.warehouse_ids.is_some() {
        if input.business_unit_ids.is_some() {
            "operating-warehouse-business-unit-scope-v1"
        } else {
            "operating-warehouse-scope-v1"
        }
    } else if input.business_unit_ids.is_some() {
        "operating-business-unit-scope-v1"
    } else {
        "operating-legal-scope-v1"
    };
    let hash = hex::encode(Sha256::digest(serde_json::to_vec(
        &json!({"version":version,"authorizationScopeHash":auth.effective_scope_hash,"scope":scopes}),
    )?));
    Ok((scopes, hash))
}
