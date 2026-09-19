use super::*;
impl ProductMasterService {
    pub(super) async fn existing_write_authority(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        actor: Uuid,
        kind: ProductMasterType,
        id: Uuid,
    ) -> Result<AuthorizationSnapshot, DomainError> {
        lock_record(tx, kind, id).await?;
        let current = crate::master_write_authority::snapshot(
            tx,
            actor,
            "business_product_master:manage",
            false,
        )
        .await?;
        let row = sqlx::query(
            "SELECT brand_id FROM product_master_data_maintenance WHERE resource_type=$1 AND id=$2",
        )
        .bind(kind.as_str())
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
        ensure_brand_scope(&current, row.get("brand_id"))?;
        Ok(current)
    }
}

pub(super) async fn lock_record(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    kind: ProductMasterType,
    id: Uuid,
) -> Result<(), DomainError> {
    let table = table(kind);
    // The maintenance view is a UNION: lock its fixed underlying table first.
    sqlx::query(AssertSqlSafe(format!(
        "SELECT id FROM {table} WHERE id=$1 FOR UPDATE"
    )))
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(DomainError::NotFoundOrForbidden)?;

    Ok(())
}

pub(super) fn table(kind: ProductMasterType) -> &'static str {
    match kind {
        ProductMasterType::UnitOfMeasure => "business_units_of_measure",
        ProductMasterType::ProductCategory => "business_product_categories",
        ProductMasterType::Brand => "business_brands",
        ProductMasterType::Product => "business_products",
        ProductMasterType::Sku => "business_skus",
        ProductMasterType::UomConversion => "business_product_uom_conversions",
    }
}
