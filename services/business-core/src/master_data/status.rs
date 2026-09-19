use super::*;

impl CoreMasterDataService {
    pub(crate) async fn change_status_on(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        context: (Uuid, Uuid),
        target: (CoreMasterType, Uuid),
        key: &str,
        input: &ChangeCoreMasterStatus,
        guard: Option<&serde_json::Value>,
    ) -> Result<CoreMasterCommandResult, DomainError> {
        let (actor, trace_id) = context;
        let (kind, id) = target;
        if !matches!(input.status.as_str(), "active" | "disabled") {
            return Err(DomainError::Invalid(
                "status must be active or disabled".into(),
            ));
        }
        crate::master_write_authority::read(tx, actor, "business_master_data:manage").await?;
        let hash = match guard {
            Some(expected) => request_hash(&(
                "guarded-master-status-v1",
                kind.as_str(),
                id,
                input,
                expected,
            ))?,
            None => request_hash(&(kind.as_str(), id, input))?,
        };
        if let Some(mut replay) = begin_idempotent::<CoreMasterCommandResult>(
            tx,
            actor,
            "core_master_data:status",
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
            let command = CoreMasterCommand::ChangeStatus {
                resource_type: kind.as_str().into(),
                document_id: id,
                command: input.clone(),
            };
            if self.preview_on(tx, actor, &command).await? != *expected {
                return Err(DomainError::StalePreview);
            }
        }
        let snapshot = self.existing_write_authority(tx, actor, kind, id).await?;
        let row=sqlx::query("SELECT code,status,version,legal_entity_id,business_unit_id FROM core_master_data_maintenance WHERE resource_type=$1 AND id=$2").bind(kind.as_str()).bind(id).fetch_optional(&mut **tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
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
        if input.status == "disabled" {
            let impacts = load_impacts_on(tx, kind, id).await?;
            if impacts.iter().any(|item| item.blocking && item.count > 0) {
                return Err(DomainError::Invalid(
                    "master data has blocking operational impacts".into(),
                ));
            }
        }
        update_status(tx, kind, id, &input.status).await?;
        let version = input.expected_version + 1;
        record(
            tx,
            trace_id,
            actor,
            "CORE_MASTER_DATA_STATUS_CHANGED",
            "core_master_data_status_changed",
            kind.as_str(),
            id,
            json!({"status":input.status,"version":version}),
        )
        .await?;
        let result = CoreMasterCommandResult {
            id,
            resource_type: kind.as_str().into(),
            code: row.get("code"),
            status: input.status.clone(),
            version,
            trace_id,
            idempotent_replay: false,
        };
        finish_idempotent(tx, actor, "core_master_data:status", key, &result).await?;
        Ok(result)
    }
}
