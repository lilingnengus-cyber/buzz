//! Reversal confirmation freezes original facts and the human-provided reason.
use super::*;
use crate::b4::model::VersionCommand;
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::document_approval::adjustment) struct Command {
    batch_id: Uuid,
    expected_version: i64,
    reason: String,
}
impl Command {
    pub(super) fn parse(value: Value) -> Result<Self, StoreError> {
        let input: Self = serde_json::from_value(value)
            .map_err(|_| StoreError::Invalid("adjustment reversal input".into()))?;
        if input.batch_id.is_nil()
            || input.expected_version < 1
            || input.reason.trim().is_empty()
            || input.reason.chars().count() > 1000
        {
            return Err(StoreError::Invalid("adjustment reversal input".into()));
        }
        Ok(input)
    }
    pub(super) fn value(&self) -> Result<Value, StoreError> {
        serde_json::to_value(self)
            .map_err(|_| StoreError::Invalid("adjustment reversal input".into()))
    }
    pub(super) async fn preview_on(
        &self,
        service: &AdjustmentService,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
    ) -> Result<Value, StoreError> {
        let preview = service
            .reversal_preview_on(
                tx,
                actor,
                self.batch_id,
                &VersionCommand {
                    expected_version: self.expected_version,
                },
                &self.reason,
            )
            .await
            .map_err(domain_error)?;
        Ok(
            json!({"schemaVersion":1,"kind":"operational_adjustment_reversal","input":self.value()?,"ownerUserId":actor,"scope":preview["preview"]["scope"],"reversalPreview":preview,"effects":preview["preview"]["effects"],"boundary":"management_only_not_general_ledger"}),
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
            .reverse_guarded_on(
                tx,
                context.0,
                context.1,
                self.batch_id,
                &format!("chat-adjustment-{request}"),
                &VersionCommand {
                    expected_version: self.expected_version,
                },
                &self.reason,
                &snapshot["reversalPreview"],
            )
            .await
            .map_err(domain_error)?;
        serde_json::to_value(result)
            .map_err(|_| StoreError::Invalid("adjustment reversal result".into()))
    }
}
