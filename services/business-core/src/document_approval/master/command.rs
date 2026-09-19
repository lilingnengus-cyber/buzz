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
            "core_master_creation_intent" | "core_master_update_intent" => Self::Core(
                serde_json::from_value(value)
                    .map_err(|_| StoreError::Invalid("core master command".into()))?,
            ),
            "product_master_creation_intent" | "product_master_update_intent" => Self::Product(
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
