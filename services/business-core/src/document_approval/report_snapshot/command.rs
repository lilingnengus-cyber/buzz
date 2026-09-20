use super::*;
use crate::b4::{model::GenerateReportSnapshot, ProfitReportingService};
use sqlx::{Postgres, Transaction};

pub(super) struct Command {
    input: GenerateReportSnapshot,
}
impl Command {
    pub(super) fn parse(kind: &str, value: Value) -> Result<Self, StoreError> {
        if kind != "management_report_snapshot_intent" {
            return Err(StoreError::NotFoundOrForbidden);
        }
        let input: GenerateReportSnapshot = serde_json::from_value(value)
            .map_err(|_| StoreError::Invalid("report snapshot input".into()))?;
        // Dimension-specific reporting is not yet represented by the source calculation.
        if input.report_type != "management_profit_statement" {
            return Err(StoreError::Invalid("unsupported agent report type".into()));
        }
        Ok(Self { input })
    }
    pub(super) fn value(&self) -> Result<Value, StoreError> {
        serde_json::to_value(&self.input)
            .map_err(|_| StoreError::Invalid("report snapshot input".into()))
    }
    pub(super) fn action(&self) -> &'static str {
        "management_report:generate_snapshot"
    }
    pub(super) async fn preview_on(
        &self,
        reporting: &ProfitReportingService,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
    ) -> Result<Value, StoreError> {
        reporting
            .snapshot_preview_on(tx, actor, &self.input)
            .await
            .map_err(domain_error)
    }
    pub(super) async fn save_on(
        &self,
        reporting: &ProfitReportingService,
        tx: &mut Transaction<'_, Postgres>,
        context: (Uuid, Uuid),
        request: Uuid,
        snapshot: &Value,
    ) -> Result<Value, StoreError> {
        let result = reporting
            .generate_snapshot_on(
                tx,
                context.0,
                context.1,
                &format!("chat-report-snapshot-{request}"),
                &self.input,
                Some(snapshot),
            )
            .await
            .map_err(domain_error)?;
        serde_json::to_value(result)
            .map_err(|_| StoreError::Invalid("report snapshot result".into()))
    }
}
fn domain_error(e: crate::b2::DomainError) -> StoreError {
    match e {
        crate::b2::DomainError::NotFoundOrForbidden => StoreError::NotFoundOrForbidden,
        crate::b2::DomainError::VersionConflict
        | crate::b2::DomainError::StalePreview
        | crate::b2::DomainError::IdempotencyConflict => StoreError::Conflict,
        crate::b2::DomainError::Database(e) => StoreError::Database(e),
        _ => StoreError::Invalid("report snapshot input or state".into()),
    }
}
