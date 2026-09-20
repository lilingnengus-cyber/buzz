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
    for (table, ids) in [
        ("business_units", &mut scopes.business_unit_ids),
        ("business_warehouses", &mut scopes.warehouse_ids),
        ("business_customers", &mut scopes.customer_ids),
        ("business_suppliers", &mut scopes.supplier_ids),
    ] {
        let allowed = ids.iter().copied().collect::<Vec<_>>();
        let legal = scopes.legal_entity_ids.iter().copied().collect::<Vec<_>>();
        let sql = format!(
            "SELECT id FROM {table} WHERE id=ANY($1) AND legal_entity_id=ANY($2) ORDER BY id"
        );
        let selected: Vec<Uuid> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .bind(allowed)
            .bind(legal)
            .fetch_all(&mut **tx)
            .await?;
        *ids = selected.into_iter().collect();
    }
    if let Some(ids) = &input.business_unit_ids {
        if ids.is_empty() {
            return Err(DomainError::Invalid(
                "businessUnitIds must not be empty".into(),
            ));
        }
        if ids.iter().any(|id| !scopes.business_unit_ids.contains(id)) {
            return Err(DomainError::NotFoundOrForbidden);
        }
        scopes.business_unit_ids = ids.iter().copied().collect();
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
