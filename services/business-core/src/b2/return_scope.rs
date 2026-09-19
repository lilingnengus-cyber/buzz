//! Additional source-order and product scopes shared by return mutations.
use super::DomainError;
use crate::store::PgStore;
use sqlx::Row;
use uuid::Uuid;

pub(super) async fn check_source(
    store: &PgStore,
    actor: Uuid,
    sales: bool,
    source: Uuid,
) -> Result<(), DomainError> {
    let (header_sql, brands_sql) = if sales {
        (
            "SELECT o.business_unit_id,o.brand_id FROM shipments s JOIN sales_orders o ON o.id=s.sales_order_id WHERE s.id=$1",
            "SELECT ol.brand_id,p.brand_id current_brand_id FROM shipment_lines l JOIN sales_order_lines ol ON ol.id=l.sales_order_line_id JOIN business_skus sku ON sku.id=l.sku_id JOIN business_products p ON p.id=sku.product_id WHERE l.shipment_id=$1",
        )
    } else {
        (
            "SELECT o.business_unit_id,o.brand_id FROM goods_receipts s JOIN purchase_orders o ON o.id=s.purchase_order_id WHERE s.id=$1",
            "SELECT ol.brand_id,p.brand_id current_brand_id FROM goods_receipt_lines l JOIN purchase_order_lines ol ON ol.id=l.purchase_order_line_id JOIN business_skus sku ON sku.id=l.sku_id JOIN business_products p ON p.id=sku.product_id WHERE l.goods_receipt_id=$1",
        )
    };
    let authority = store
        .snapshot(actor)
        .await
        .map_err(|_| DomainError::NotFoundOrForbidden)?;
    let header = sqlx::query(header_sql)
        .bind(source)
        .fetch_optional(store.pool())
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
    if !authority
        .scopes
        .business_unit_ids
        .contains(&header.get("business_unit_id"))
        || header
            .get::<Option<Uuid>, _>("brand_id")
            .is_some_and(|id| !authority.scopes.brand_ids.contains(&id))
    {
        return Err(DomainError::NotFoundOrForbidden);
    }
    let lines = sqlx::query(brands_sql)
        .bind(source)
        .fetch_all(store.pool())
        .await?;
    if lines.is_empty()
        || lines.iter().any(|line| {
            ["brand_id", "current_brand_id"].iter().any(|column| {
                line.get::<Option<Uuid>, _>(*column)
                    .is_some_and(|id| !authority.scopes.brand_ids.contains(&id))
            })
        })
    {
        return Err(DomainError::NotFoundOrForbidden);
    }
    Ok(())
}

pub(super) async fn check_return(
    store: &PgStore,
    actor: Uuid,
    sales: bool,
    id: Uuid,
) -> Result<(), DomainError> {
    let sql = if sales {
        "SELECT shipment_id FROM sales_returns WHERE id=$1"
    } else {
        "SELECT goods_receipt_id FROM purchase_returns WHERE id=$1"
    };
    let source = sqlx::query_scalar(sql)
        .bind(id)
        .fetch_optional(store.pool())
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
    check_source(store, actor, sales, source).await
}
