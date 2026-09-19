//! Current warehouse business-unit and per-SKU brand authority for counts.
use super::DomainError;
use crate::{model::AuthorizationSnapshot, store::PgStore};
use uuid::Uuid;

pub(super) async fn check(
    store: &PgStore,
    authority: &AuthorizationSnapshot,
    warehouse: Uuid,
    skus: &[Uuid],
) -> Result<(), DomainError> {
    let unit: Uuid =
        sqlx::query_scalar("SELECT business_unit_id FROM business_warehouses WHERE id=$1")
            .bind(warehouse)
            .fetch_optional(store.pool())
            .await?
            .ok_or(DomainError::NotFoundOrForbidden)?;
    if !authority.scopes.business_unit_ids.contains(&unit) || skus.is_empty() {
        return Err(DomainError::NotFoundOrForbidden);
    }
    let brands: Vec<Option<Uuid>> = sqlx::query_scalar(
        "SELECT p.brand_id FROM business_skus s JOIN business_products p ON p.id=s.product_id WHERE s.id=ANY($1)",
    )
    .bind(skus)
    .fetch_all(store.pool())
    .await?;
    if brands.len() != skus.len()
        || brands
            .into_iter()
            .flatten()
            .any(|id| !authority.scopes.brand_ids.contains(&id))
    {
        return Err(DomainError::NotFoundOrForbidden);
    }
    Ok(())
}
