use super::*;

impl ProductMasterService {
    pub(crate) async fn change_status_on(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        context: (Uuid, Uuid),
        target: (ProductMasterType, Uuid),
        key: &str,
        input: &ChangeProductMasterStatus,
        guard: Option<&serde_json::Value>,
    ) -> Result<ProductMasterCommandResult, DomainError> {
        let (actor, trace_id) = context;
        let (kind, id) = target;
        if !matches!(input.status.as_str(), "active" | "disabled") {
            return Err(DomainError::Invalid(
                "status must be active or disabled".into(),
            ));
        }
        crate::master_write_authority::read(tx, actor, "business_product_master:manage").await?;
        let hash = match guard {
            Some(expected) => request_hash(&(
                "guarded-product-status-v1",
                kind.as_str(),
                id,
                input,
                expected,
            ))?,
            None => request_hash(&(kind.as_str(), id, input))?,
        };
        if let Some(mut replay) = begin_idempotent::<ProductMasterCommandResult>(
            tx,
            actor,
            "product_master_data:status",
            key,
            &hash,
        )
        .await?
        {
            self.existing_write_authority(tx, actor, kind, replay.id)
                .await?;
            replay.idempotent_replay = true;
            return Ok(replay);
        }
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("{}:{id}", kind.as_str()))
            .execute(&mut **tx)
            .await?;
        if let Some(expected) = guard {
            let command = ProductMasterCommand::ChangeStatus {
                resource_type: kind.as_str().into(),
                document_id: id,
                command: input.clone(),
            };
            if self.preview_on(tx, actor, &command).await? != *expected {
                return Err(DomainError::StalePreview);
            }
        }
        let snapshot = self.existing_write_authority(tx, actor, kind, id).await?;
        let row = load_record(tx, kind, id)
            .await?
            .ok_or(DomainError::NotFoundOrForbidden)?;
        ensure_brand_scope(&snapshot, row.get("brand_id"))?;
        if row.get::<i64, _>("version") != input.expected_version {
            return Err(DomainError::VersionConflict);
        }
        if input.status == "disabled" {
            let impacts = load_impacts_on(tx, kind, id).await?;
            if impacts.iter().any(|item| item.blocking && item.count > 0) {
                return Err(DomainError::Invalid(
                    "product master data has blocking operational impacts".into(),
                ));
            }
        } else {
            ensure_enable_dependencies(tx, kind, id).await?;
        }
        update_status(tx, kind, id, &input.status).await?;
        let version = input.expected_version + 1;
        let code: String = row.get("code");
        record(
            tx,
            trace_id,
            actor,
            "PRODUCT_MASTER_DATA_STATUS_CHANGED",
            "product_master_data_status_changed",
            kind.as_str(),
            id,
            json!({"status":input.status,"version":version}),
        )
        .await?;
        let result = ProductMasterCommandResult {
            id,
            resource_type: kind.as_str().into(),
            code,
            status: input.status.clone(),
            version,
            trace_id,
            idempotent_replay: false,
        };
        finish_idempotent(tx, actor, "product_master_data:status", key, &result).await?;
        Ok(result)
    }
}
