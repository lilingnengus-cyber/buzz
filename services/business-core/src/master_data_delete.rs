use super::*;

impl CoreMasterDataService {
    /// Delete an unreferenced record atomically with its audit and outbox events.
    pub async fn delete(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        kind: CoreMasterType,
        id: Uuid,
        key: &str,
        input: &DeleteCoreMasterData,
    ) -> Result<CoreMasterCommandResult, DomainError> {
        let snapshot = self.snapshot(actor, "business_master_data:manage").await?;
        let hash = request_hash(&(kind.as_str(), id, input))?;
        let mut tx = self.store.pool().begin().await?;
        if let Some(mut replay) = begin_idempotent::<CoreMasterCommandResult>(
            &mut tx,
            actor,
            "core_master_data:delete",
            key,
            &hash,
        )
        .await?
        {
            replay.idempotent_replay = true;
            tx.commit().await?;
            return Ok(replay);
        }
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("{}:{id}", kind.as_str()))
            .execute(&mut *tx)
            .await?;
        let row = sqlx::query("SELECT code,version,legal_entity_id,business_unit_id FROM core_master_data_maintenance WHERE resource_type=$1 AND id=$2")
            .bind(kind.as_str()).bind(id).fetch_optional(&mut *tx).await?
            .ok_or(DomainError::NotFoundOrForbidden)?;
        self.ensure_scope(
            &snapshot,
            kind,
            row.get("legal_entity_id"),
            row.get("business_unit_id"),
            id,
        )?;
        if row.get::<i64, _>("version") != input.expected_version {
            return Err(DomainError::VersionConflict);
        }
        let table = match kind {
            CoreMasterType::LegalEntity => "business_legal_entities",
            CoreMasterType::BusinessUnit => "business_units",
            CoreMasterType::Customer => "business_customers",
            CoreMasterType::Supplier => "business_suppliers",
            CoreMasterType::Warehouse => "business_warehouses",
        };
        // Foreign keys protect all historical references, including concurrent inserts.
        // Authorization scopes and user preferences cascade; business facts restrict deletion.
        let deleted = sqlx::query(AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE id=$1 AND version=$2"
        )))
        .bind(id)
        .bind(input.expected_version)
        .execute(&mut *tx)
        .await
        .map_err(|error| {
            if error
                .as_database_error()
                .is_some_and(|db| db.is_foreign_key_violation())
            {
                DomainError::Invalid(
                    "该记录已被下级数据或业务记录引用，无法删除；请改用停用。".into(),
                )
            } else {
                DomainError::from(error)
            }
        })?;
        if deleted.rows_affected() != 1 {
            return Err(DomainError::VersionConflict);
        }
        let result = CoreMasterCommandResult {
            id,
            resource_type: kind.as_str().into(),
            code: row.get("code"),
            status: "deleted".into(),
            version: input.expected_version + 1,
            trace_id,
            idempotent_replay: false,
        };
        record(
            &mut tx,
            trace_id,
            actor,
            "CORE_MASTER_DATA_DELETED",
            "core_master_data_deleted",
            kind.as_str(),
            id,
            json!({"code":result.code,"version":result.version}),
        )
        .await?;
        finish_idempotent(&mut tx, actor, "core_master_data:delete", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
}
