use super::*;
use crate::b4::{model::VersionCommand, AdjustmentService};
use sqlx::{Postgres, Transaction};
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    batch_id: Uuid,
    expected_version: i64,
}
pub(in crate::document_approval::adjustment) struct Command {
    input: Input,
}
impl Command {
    pub(super) fn parse(kind: &str, value: Value) -> Result<Self, StoreError> {
        if kind != "operational_adjustment_post_intent" {
            return Err(StoreError::NotFoundOrForbidden);
        }
        let input: Input = serde_json::from_value(value)
            .map_err(|_| StoreError::Invalid("adjustment posting input".into()))?;
        if input.expected_version < 1 {
            return Err(StoreError::Invalid("adjustment version".into()));
        }
        Ok(Self { input })
    }
    pub(super) fn value(&self) -> Result<Value, StoreError> {
        serde_json::to_value(&self.input)
            .map_err(|_| StoreError::Invalid("adjustment posting input".into()))
    }
    pub(super) fn action(&self) -> &'static str {
        "profit_adjustment:post"
    }
    pub(super) async fn preview_on(
        &self,
        service: &AdjustmentService,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
    ) -> Result<Value, StoreError> {
        crate::master_write_authority::read(tx, actor, self.action())
            .await
            .map_err(domain_error)?;
        let preview = service
            .allocation_preview_on(
                tx,
                actor,
                self.input.batch_id,
                &VersionCommand {
                    expected_version: self.input.expected_version,
                },
            )
            .await
            .map_err(domain_error)?;
        Ok(
            json!({"schemaVersion":1,"kind":"operational_adjustment_post","input":self.value()?,"ownerUserId":actor,"scope":preview["preview"]["scope"],"allocationPreview":preview,"effects":{"postsAdjustment":true,"createsBatch":false},"boundary":"management_only_not_general_ledger"}),
        )
    }
    pub(super) async fn save_on(
        &self,
        service: &AdjustmentService,
        tx: &mut Transaction<'_, Postgres>,
        context: (Uuid, Uuid),
        request: Uuid,
        snapshot: &Value,
    ) -> Result<Value, StoreError> {
        let result = service
            .post_guarded_on(
                tx,
                context.0,
                context.1,
                self.input.batch_id,
                &format!("chat-adjustment-{request}"),
                &VersionCommand {
                    expected_version: self.input.expected_version,
                },
                &snapshot["allocationPreview"],
            )
            .await
            .map_err(domain_error)?;
        serde_json::to_value(result)
            .map_err(|_| StoreError::Invalid("adjustment posting result".into()))
    }
}
pub(crate) fn domain_error(e: crate::b2::DomainError) -> StoreError {
    match e {
        crate::b2::DomainError::NotFoundOrForbidden => StoreError::NotFoundOrForbidden,
        crate::b2::DomainError::VersionConflict
        | crate::b2::DomainError::StalePreview
        | crate::b2::DomainError::IdempotencyConflict => StoreError::Conflict,
        crate::b2::DomainError::Database(e) => StoreError::Database(e),
        _ => StoreError::Invalid("adjustment input or state".into()),
    }
}
