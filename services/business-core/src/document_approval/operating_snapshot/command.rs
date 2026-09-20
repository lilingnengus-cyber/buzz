use super::*;
use crate::s1::{GenerateOperatingSnapshot, OperationsService};
use sqlx::{Postgres, Transaction};

pub(super) struct Command {
    input: GenerateOperatingSnapshot,
}
impl Command {
    pub(super) fn parse(kind: &str, value: Value) -> Result<Self, StoreError> {
        if kind != "operating_report_snapshot_intent" {
            return Err(StoreError::NotFoundOrForbidden);
        }
        let input: GenerateOperatingSnapshot = serde_json::from_value(value)
            .map_err(|_| StoreError::Invalid("operating snapshot input".into()))?;
        Ok(Self { input })
    }
    pub(super) fn value(&self) -> Result<Value, StoreError> {
        serde_json::to_value(&self.input)
            .map_err(|_| StoreError::Invalid("operating snapshot input".into()))
    }
    pub(super) fn action(&self) -> &'static str {
        "management_report:generate_snapshot"
    }
    pub(super) async fn preview_on(
        &self,
        reporting: &OperationsService,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
    ) -> Result<Value, StoreError> {
        reporting
            .operating_snapshot_preview_on(tx, actor, &self.input)
            .await
            .map_err(domain_error)
    }
    pub(super) async fn save_on(
        &self,
        reporting: &OperationsService,
        tx: &mut Transaction<'_, Postgres>,
        context: (Uuid, Uuid),
        request: Uuid,
        snapshot: &Value,
    ) -> Result<Value, StoreError> {
        let result = reporting
            .generate_operating_snapshot_guarded_on(
                tx,
                context.0,
                context.1,
                &format!("chat-operating-snapshot-{request}"),
                &self.input,
                snapshot,
            )
            .await
            .map_err(domain_error)?;
        serde_json::to_value(result)
            .map_err(|_| StoreError::Invalid("operating snapshot result".into()))
    }
}
pub(super) fn domain_error(e: crate::b2::DomainError) -> StoreError {
    match e {
        crate::b2::DomainError::NotFoundOrForbidden => StoreError::NotFoundOrForbidden,
        crate::b2::DomainError::VersionConflict
        | crate::b2::DomainError::StalePreview
        | crate::b2::DomainError::IdempotencyConflict => StoreError::Conflict,
        crate::b2::DomainError::Database(e) => StoreError::Database(e),
        _ => StoreError::Invalid("operating snapshot input or state".into()),
    }
}
