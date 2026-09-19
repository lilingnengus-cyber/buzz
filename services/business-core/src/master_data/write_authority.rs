use super::*;
impl CoreMasterDataService {
    pub(super) async fn existing_write_authority(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        actor: Uuid,
        kind: CoreMasterType,
        id: Uuid,
    ) -> Result<AuthorizationSnapshot, DomainError> {
        lock_record(tx, kind, id).await?;
        let current = crate::master_write_authority::snapshot(
            tx,
            actor,
            "business_master_data:manage",
            false,
        )
        .await?;
        let row = sqlx::query("SELECT legal_entity_id,business_unit_id FROM core_master_data_maintenance WHERE resource_type=$1 AND id=$2")
            .bind(kind.as_str()).bind(id).fetch_optional(&mut **tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
        self.ensure_scope(
            &current,
            kind,
            row.get("legal_entity_id"),
            row.get("business_unit_id"),
            id,
        )?;
        Ok(current)
    }
}

pub(super) async fn lock_record(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    kind: CoreMasterType,
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

pub(super) fn table(kind: CoreMasterType) -> &'static str {
    match kind {
        CoreMasterType::LegalEntity => "business_legal_entities",
        CoreMasterType::BusinessUnit => "business_units",
        CoreMasterType::Customer => "business_customers",
        CoreMasterType::Supplier => "business_suppliers",
        CoreMasterType::Warehouse => "business_warehouses",
    }
}
