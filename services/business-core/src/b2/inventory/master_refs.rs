//! Current master status locks shared by opening entry and posting.
use super::*;

pub(super) async fn lock_active(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    legal: Uuid,
    warehouse: Uuid,
    sku: Uuid,
) -> Result<bool, DomainError> {
    let row=sqlx::query("SELECT p.allow_zero_cost,p.brand_id FROM business_warehouses w JOIN business_legal_entities e ON e.id=w.legal_entity_id JOIN business_units bu ON bu.id=w.business_unit_id JOIN business_skus s ON s.id=$3 JOIN business_products p ON p.id=s.product_id JOIN business_units_of_measure u ON u.id=p.base_uom_id JOIN business_product_categories c ON c.id=p.category_id WHERE w.id=$2 AND e.id=$1 AND bu.legal_entity_id=e.id AND e.status='active' AND bu.status='active' AND w.status='active' AND s.status='active' AND p.status='active' AND u.status='active' AND c.status='active' FOR SHARE OF e,bu,w,s,p,u,c")
        .bind(legal).bind(warehouse).bind(sku).fetch_optional(&mut **tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
    if let Some(brand) = row.get::<Option<Uuid>, _>("brand_id") {
        sqlx::query("SELECT id FROM business_brands WHERE id=$1 AND status='active' FOR SHARE")
            .bind(brand)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or(DomainError::NotFoundOrForbidden)?;
    }
    Ok(row.get("allow_zero_cost"))
}
