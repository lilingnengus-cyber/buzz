use super::*;
use crate::master_command::{parent, unchanged, MasterCommand};
use serde_json::Value;
use sqlx::{Postgres, Transaction};

/// Exact Core master create, replacement or status command.
pub type CoreMasterCommand = MasterCommand<SaveCoreMasterData, ChangeCoreMasterStatus>;

impl CoreMasterDataService {
    /// Read the full current record for authorized maintenance or command preparation.
    pub async fn detail(
        &self,
        actor: Uuid,
        kind: CoreMasterType,
        id: Uuid,
    ) -> Result<CoreMasterRecord, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        let scope = PgStore::snapshot_on(&mut tx, actor)
            .await
            .map_err(|_| DomainError::NotFoundOrForbidden)?;
        if !scope.permission_keys.contains("business_master_data:read")
            && !scope
                .permission_keys
                .contains("business_master_data:manage")
        {
            return Err(DomainError::NotFoundOrForbidden);
        }
        let record = read_record(&mut tx, kind, id).await?;
        self.ensure_scope(
            &scope,
            kind,
            record.legal_entity_id,
            record.business_unit_id,
            id,
        )?;
        tx.rollback().await?;
        Ok(record)
    }
    /// Read a stable command preview without saving an intent or changing business state.
    pub async fn command_preview(
        &self,
        actor: Uuid,
        command: &CoreMasterCommand,
    ) -> Result<Value, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        let value = self.preview_on(&mut tx, actor, command).await?;
        tx.rollback().await?;
        Ok(value)
    }
    /// Execute a command only if its preview still matches in the write transaction.
    /// This is a domain consistency guard, not approval authorization.
    pub async fn save_guarded(
        &self,
        actor: Uuid,
        trace: Uuid,
        key: &str,
        command: &CoreMasterCommand,
        snapshot: &Value,
    ) -> Result<CoreMasterCommandResult, DomainError> {
        match command {
            MasterCommand::Create { command } => {
                self.save_inner((actor, trace), None, key, command, Some(snapshot))
                    .await
            }
            MasterCommand::Update {
                document_id,
                command,
            } => {
                self.save_inner(
                    (actor, trace),
                    Some(*document_id),
                    key,
                    command,
                    Some(snapshot),
                )
                .await
            }
            MasterCommand::ChangeStatus {
                resource_type,
                document_id,
                command,
            } => {
                let kind = CoreMasterType::from_str(resource_type)?;
                let mut tx = self.store.pool().begin().await?;
                let result = self
                    .change_status_on(
                        &mut tx,
                        (actor, trace),
                        (kind, *document_id),
                        key,
                        command,
                        Some(snapshot),
                    )
                    .await?;
                tx.commit().await?;
                Ok(result)
            }
        }
    }

    pub(crate) async fn preview_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        command: &CoreMasterCommand,
    ) -> Result<Value, DomainError> {
        crate::master_write_authority::read(tx, actor, "business_master_data:manage").await?;
        let (kind, id, expected, save, status) = match command {
            MasterCommand::Create { command: input } => {
                let kind = CoreMasterType::from_str(&input.resource_type)?;
                validate(input, kind, false)?;
                applicable(input, kind)?;
                if input.expected_version.is_some() {
                    return Err(DomainError::VersionConflict);
                }
                (kind, None, None, Some(input), None)
            }
            MasterCommand::Update {
                document_id,
                command: input,
            } => {
                let kind = CoreMasterType::from_str(&input.resource_type)?;
                validate(input, kind, true)?;
                applicable(input, kind)?;
                if input.expected_version.is_none_or(|v| v < 1) {
                    return Err(DomainError::VersionConflict);
                }
                (
                    kind,
                    Some(*document_id),
                    input.expected_version,
                    Some(input),
                    None,
                )
            }
            MasterCommand::ChangeStatus {
                resource_type,
                document_id,
                command: input,
            } => {
                if input.expected_version < 1
                    || !matches!(input.status.as_str(), "active" | "disabled")
                {
                    return Err(DomainError::Invalid("invalid master status/version".into()));
                }
                (
                    CoreMasterType::from_str(resource_type)?,
                    Some(*document_id),
                    Some(input.expected_version),
                    None,
                    Some(input),
                )
            }
        };
        let mut current = if let Some(id) = id {
            write_authority::lock_record(tx, kind, id).await?;
            Some(read_record(tx, kind, id).await?)
        } else {
            None
        };
        if let Some(old) = &current {
            if Some(old.version) != expected {
                return Err(DomainError::VersionConflict);
            }
            if let Some(input) = save {
                unchanged(old.code.as_str(), input.code.as_str())?;
                if kind != CoreMasterType::LegalEntity {
                    unchanged(old.legal_entity_id, input.legal_entity_id)?;
                }
                if !matches!(
                    kind,
                    CoreMasterType::LegalEntity | CoreMasterType::BusinessUnit
                ) {
                    unchanged(old.business_unit_id, input.business_unit_id)?;
                }
            }
        }
        let legal = current
            .as_ref()
            .and_then(|v| v.legal_entity_id)
            .or_else(|| save.and_then(|v| v.legal_entity_id));
        let unit = current
            .as_ref()
            .and_then(|v| v.business_unit_id)
            .or_else(|| save.and_then(|v| v.business_unit_id));
        let mut parents = serde_json::Map::new();
        if !matches!(
            kind,
            CoreMasterType::LegalEntity | CoreMasterType::BusinessUnit
        ) {
            let legal = legal.ok_or(DomainError::NotFoundOrForbidden)?;
            parents.insert(
                "legalEntity".into(),
                parent(tx, "business_legal_entities", legal).await?,
            );
        }
        if !matches!(
            kind,
            CoreMasterType::LegalEntity | CoreMasterType::BusinessUnit
        ) {
            let unit = unit.ok_or(DomainError::NotFoundOrForbidden)?;
            let record = parent(tx, "business_units", unit).await?;
            parents.insert("businessUnit".into(), record);
        }
        // Re-read joined display fields only after all their parent rows are locked.
        if let Some(id) = id {
            current = Some(read_record(tx, kind, id).await?);
        }
        let scope = crate::master_write_authority::snapshot(
            tx,
            actor,
            "business_master_data:manage",
            id.is_none(),
        )
        .await?;
        if let Some(old) = &current {
            self.ensure_scope(
                &scope,
                kind,
                old.legal_entity_id,
                old.business_unit_id,
                old.id,
            )?;
        } else if legal.is_some_and(|v| !scope.scopes.legal_entity_ids.contains(&v))
            || unit.is_some_and(|v| !scope.scopes.business_unit_ids.contains(&v))
        {
            return Err(DomainError::NotFoundOrForbidden);
        }
        if (id.is_none() || status.is_some_and(|v| v.status == "active"))
            && parents.values().any(|v| v["status"] != "active")
        {
            return Err(DomainError::Invalid(
                "parent master data must be active".into(),
            ));
        }
        if id.is_none() {
            if let Some(input) = save {
                let table = write_authority::table(kind);
                let used: bool = sqlx::query_scalar(AssertSqlSafe(format!(
                    "SELECT EXISTS(SELECT 1 FROM {table} WHERE code=$1)"
                )))
                .bind(&input.code)
                .fetch_one(&mut **tx)
                .await?;
                if used {
                    return Err(DomainError::Invalid("master code is already in use".into()));
                }
            }
        }
        let impacts = if let Some(id) = id {
            load_impacts_on(tx, kind, id).await?
        } else {
            Vec::new()
        };
        let can_execute = status.is_none_or(|v| v.status != "disabled")
            || !impacts.iter().any(|v| v.blocking && v.count > 0);
        let effective = save
            .map(|v| effective_fields(v, kind))
            .unwrap_or_else(|| json!({"status":status.map(|v|&v.status)}));
        Ok(
            json!({"documentType":format!("core_master_{}_intent",command.intent_suffix()),"resourceType":kind.as_str(),"documentId":id,
            "legalEntityId":legal,"businessUnitId":unit,"current":current,"parents":parents,"command":command,"effectiveFields":effective,
            "disableImpacts":impacts,"canExecute":can_execute}),
        )
    }
}
async fn read_record(
    tx: &mut Transaction<'_, Postgres>,
    kind: CoreMasterType,
    id: Uuid,
) -> Result<CoreMasterRecord, DomainError> {
    sqlx::query_as("SELECT * FROM core_master_data_maintenance WHERE resource_type=$1 AND id=$2")
        .bind(kind.as_str())
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)
}
fn applicable(v: &SaveCoreMasterData, kind: CoreMasterType) -> Result<(), DomainError> {
    let invalid = (kind != CoreMasterType::LegalEntity
        && (v.country_code.is_some()
            || v.functional_currency.is_some()
            || v.registration_number.is_some()))
        || (kind == CoreMasterType::LegalEntity
            && (v.legal_entity_id.is_some() || v.business_unit_id.is_some()))
        || (kind == CoreMasterType::BusinessUnit && v.business_unit_id.is_some())
        || (kind != CoreMasterType::Warehouse && v.address.is_some())
        || (kind != CoreMasterType::Customer
            && (v.credit_currency.is_some() || v.credit_limit_minor.is_some()))
        || (!matches!(kind, CoreMasterType::Customer | CoreMasterType::Supplier)
            && v.payment_terms_days.is_some());
    if invalid {
        return Err(DomainError::Invalid(
            "fields do not apply to this master type".into(),
        ));
    }
    Ok(())
}
fn effective_fields(v: &SaveCoreMasterData, kind: CoreMasterType) -> Value {
    let mut fields = json!({"code":v.code,"name":v.name.trim()});
    let extra = match kind {
        CoreMasterType::LegalEntity => {
            json!({"countryCode":v.country_code,"functionalCurrency":v.functional_currency,"registrationNumber":v.registration_number})
        }
        CoreMasterType::BusinessUnit => json!({"legalEntityId":v.legal_entity_id}),
        CoreMasterType::Customer => {
            json!({"legalEntityId":v.legal_entity_id,"businessUnitId":v.business_unit_id,"creditCurrency":v.credit_currency,"creditLimitMinor":v.credit_limit_minor.unwrap_or(0),"paymentTermsDays":v.payment_terms_days.unwrap_or(30)})
        }
        CoreMasterType::Supplier => {
            json!({"legalEntityId":v.legal_entity_id,"businessUnitId":v.business_unit_id,"paymentTermsDays":v.payment_terms_days.unwrap_or(30)})
        }
        CoreMasterType::Warehouse => {
            json!({"legalEntityId":v.legal_entity_id,"businessUnitId":v.business_unit_id,"address":v.address})
        }
    };
    if let (Some(fields), Some(extra)) = (fields.as_object_mut(), extra.as_object()) {
        fields.extend(extra.clone());
    }
    fields
}
