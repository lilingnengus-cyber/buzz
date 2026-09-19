use super::*;
use crate::{
    master_command::MasterCommand,
    master_data::{CoreMasterCommand, CoreMasterDataService},
    product_master::{ProductMasterCommand, ProductMasterService},
};

pub(super) enum Command {
    Core(CoreMasterCommand),
    Product(ProductMasterCommand),
}
impl Command {
    pub(super) fn parse(kind: &str, value: Value) -> Result<Self, StoreError> {
        let command = match kind {
            "core_master_creation_intent"
            | "core_master_update_intent"
            | "core_master_status_intent" => Self::Core(
                serde_json::from_value(value)
                    .map_err(|_| StoreError::Invalid("core master command".into()))?,
            ),
            "product_master_creation_intent"
            | "product_master_update_intent"
            | "product_master_status_intent" => Self::Product(
                serde_json::from_value(value)
                    .map_err(|_| StoreError::Invalid("product master command".into()))?,
            ),
            _ => return Err(StoreError::NotFoundOrForbidden),
        };
        let suffix = match &command {
            Self::Core(c) => c.intent_suffix(),
            Self::Product(c) => c.intent_suffix(),
        };
        if !kind.ends_with(&format!("_{suffix}_intent")) {
            return Err(StoreError::Invalid(
                "master command does not match intent kind".into(),
            ));
        }
        Ok(command)
    }
    pub(super) fn value(&self) -> Result<Value, StoreError> {
        match self {
            Self::Core(c) => serde_json::to_value(c),
            Self::Product(c) => serde_json::to_value(c),
        }
        .map_err(|_| StoreError::Invalid("master command".into()))
    }
    pub(super) async fn lock_target(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), StoreError> {
        let target = match self {
            Self::Core(MasterCommand::Update {
                document_id,
                command,
            }) => Some((&command.resource_type, document_id)),
            Self::Product(MasterCommand::Update {
                document_id,
                command,
            }) => Some((&command.resource_type, document_id)),
            Self::Core(MasterCommand::ChangeStatus {
                resource_type,
                document_id,
                ..
            })
            | Self::Product(MasterCommand::ChangeStatus {
                resource_type,
                document_id,
                ..
            }) => Some((resource_type, document_id)),
            _ => None,
        };
        if let Some((kind, id)) = target {
            // Same order as normal saves: advisory target, physical row, authority.
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
                .bind(format!("{kind}:{id}"))
                .execute(&mut **tx)
                .await?;
        }
        Ok(())
    }
    pub(super) fn action(&self) -> &'static str {
        match self {
            Self::Core(_) => "business_master_data:manage",
            Self::Product(_) => "business_product_master:manage",
        }
    }
    pub(super) async fn preview_on(
        &self,
        store: &PgStore,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        actor: Uuid,
    ) -> Result<Value, StoreError> {
        match self {
            Self::Core(c) => {
                CoreMasterDataService::new(store.clone())
                    .preview_on(tx, actor, c)
                    .await
            }
            Self::Product(c) => {
                ProductMasterService::new(store.clone())
                    .preview_on(tx, actor, c)
                    .await
            }
        }
        .map_err(domain_error)
    }
    pub(super) fn creation(&self) -> bool {
        matches!(
            self,
            Self::Core(MasterCommand::Create { .. }) | Self::Product(MasterCommand::Create { .. })
        )
    }
    pub(super) async fn grant_requester(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        requester: Uuid,
        approver: Uuid,
        result: &Value,
    ) -> Result<(), StoreError> {
        let kind = result["resourceType"]
            .as_str()
            .ok_or(StoreError::Conflict)?;
        let (table, column) = match kind {
            "legal_entity" => ("business_legal_entity_scopes", "legal_entity_id"),
            "business_unit" => ("business_unit_scopes", "business_unit_id"),
            "customer" => ("business_customer_scopes", "customer_id"),
            "supplier" => ("business_supplier_scopes", "supplier_id"),
            "warehouse" => ("business_warehouse_scopes", "warehouse_id"),
            "brand" => ("business_brand_scopes", "brand_id"),
            _ => return Ok(()),
        };
        let id = result["id"]
            .as_str()
            .and_then(|v| Uuid::parse_str(v).ok())
            .ok_or(StoreError::Conflict)?;
        // Only the newly created object: existing parent permissions are never restored.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {table}(enterprise_user_id,{column},granted_by) VALUES($1,$2,$3)"
        )))
        .bind(requester)
        .bind(id)
        .bind(approver)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }
    pub(super) async fn save_on(
        &self,
        store: &PgStore,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        context: (Uuid, Uuid),
        request: Uuid,
        snapshot: &Value,
    ) -> Result<Value, StoreError> {
        let key = format!("agent-master:{request}");
        match self {
            Self::Core(MasterCommand::ChangeStatus {
                resource_type,
                document_id,
                command,
            }) => {
                let kind = resource_type.parse().map_err(domain_error)?;
                serde_json::to_value(
                    CoreMasterDataService::new(store.clone())
                        .change_status_on(
                            tx,
                            context,
                            (kind, *document_id),
                            &key,
                            command,
                            Some(snapshot),
                        )
                        .await
                        .map_err(domain_error)?,
                )
            }
            Self::Product(MasterCommand::ChangeStatus {
                resource_type,
                document_id,
                command,
            }) => {
                let kind = resource_type.parse().map_err(domain_error)?;
                serde_json::to_value(
                    ProductMasterService::new(store.clone())
                        .change_status_on(
                            tx,
                            context,
                            (kind, *document_id),
                            &key,
                            command,
                            Some(snapshot),
                        )
                        .await
                        .map_err(domain_error)?,
                )
            }

            Self::Core(c) => {
                let (id, command) = match c {
                    MasterCommand::Create { command } => (None, command),
                    MasterCommand::Update {
                        document_id,
                        command,
                    } => (Some(*document_id), command),
                    _ => return Err(StoreError::NotFoundOrForbidden),
                };
                serde_json::to_value(
                    CoreMasterDataService::new(store.clone())
                        .save_on(tx, context, id, &key, command, Some(snapshot))
                        .await
                        .map_err(domain_error)?,
                )
            }
            Self::Product(c) => {
                let (id, command) = match c {
                    MasterCommand::Create { command } => (None, command),
                    MasterCommand::Update {
                        document_id,
                        command,
                    } => (Some(*document_id), command),
                    _ => return Err(StoreError::NotFoundOrForbidden),
                };
                serde_json::to_value(
                    ProductMasterService::new(store.clone())
                        .save_on(tx, context, id, &key, command, Some(snapshot))
                        .await
                        .map_err(domain_error)?,
                )
            }
        }
        .map_err(|_| StoreError::Invalid("master result".into()))
    }
}
fn domain_error(e: crate::b2::DomainError) -> StoreError {
    match e {
        crate::b2::DomainError::NotFoundOrForbidden => StoreError::NotFoundOrForbidden,
        crate::b2::DomainError::VersionConflict
        | crate::b2::DomainError::StalePreview
        | crate::b2::DomainError::IdempotencyConflict => StoreError::Conflict,
        crate::b2::DomainError::Database(e) => StoreError::Database(e),
        _ => StoreError::Invalid("master command input or state".into()),
    }
}
