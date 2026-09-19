//! Recheck the current master references before reserving order inventory.
use super::*;

pub(super) async fn lock_order(
    tx: &mut Transaction<'_, Postgres>,
    order: Uuid,
) -> Result<(), DomainError> {
    let header = sqlx::query("SELECT o.legal_entity_id FROM sales_orders o JOIN business_customers c ON c.id=o.customer_id JOIN business_units u ON u.id=o.business_unit_id WHERE o.id=$1 AND c.status='active' AND u.status='active' AND c.legal_entity_id=o.legal_entity_id AND u.legal_entity_id=o.legal_entity_id FOR SHARE OF c,u")
        .bind(order).fetch_optional(&mut **tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
    let legal: Uuid = header.get("legal_entity_id");
    let lines = sqlx::query("SELECT warehouse_id,sku_id,unit_of_measure_id FROM sales_order_lines WHERE sales_order_id=$1 ORDER BY warehouse_id,sku_id,id")
        .bind(order).fetch_all(&mut **tx).await?;
    for line in lines {
        crate::b2::stock_master_refs::lock_active(
            tx,
            legal,
            line.get("warehouse_id"),
            line.get("sku_id"),
        )
        .await?;
        let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM business_skus s JOIN business_products p ON p.id=s.product_id WHERE s.id=$1 AND p.base_uom_id=$2)")
            .bind(line.get::<Uuid,_>("sku_id")).bind(line.get::<Uuid,_>("unit_of_measure_id")).fetch_one(&mut **tx).await?;
        if !valid {
            return Err(DomainError::NotFoundOrForbidden);
        }
    }
    Ok(())
}

pub(super) async fn ready(pool: &sqlx::PgPool, order: Uuid) -> Result<bool, DomainError> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sales_orders o JOIN business_legal_entities e ON e.id=o.legal_entity_id JOIN business_customers c ON c.id=o.customer_id JOIN business_units bu ON bu.id=o.business_unit_id WHERE o.id=$1 AND e.status='active' AND c.status='active' AND bu.status='active' AND c.legal_entity_id=e.id AND bu.legal_entity_id=e.id AND NOT EXISTS(SELECT 1 FROM sales_order_lines l JOIN business_warehouses w ON w.id=l.warehouse_id JOIN business_units wu ON wu.id=w.business_unit_id JOIN business_skus s ON s.id=l.sku_id JOIN business_products p ON p.id=s.product_id JOIN business_units_of_measure u ON u.id=p.base_uom_id JOIN business_product_categories pc ON pc.id=p.category_id WHERE l.sales_order_id=o.id AND (w.status<>'active' OR wu.status<>'active' OR s.status<>'active' OR p.status<>'active' OR u.status<>'active' OR pc.status<>'active' OR w.legal_entity_id<>e.id OR wu.legal_entity_id<>e.id OR p.base_uom_id<>l.unit_of_measure_id OR (p.brand_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM business_brands b WHERE b.id=p.brand_id AND b.status='active')))))")
        .bind(order).fetch_one(pool).await?)
}
