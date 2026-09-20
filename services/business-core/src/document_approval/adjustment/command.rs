use super::*;
use crate::b4::{model::CreateAdjustmentBatch, AdjustmentService};
use sqlx::{Postgres, Transaction};
mod post;
pub(super) use post::domain_error;
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Replacement {
    batch_id: Uuid,
    expected_version: i64,
    batch: CreateAdjustmentBatch,
}
pub(super) enum Command {
    Post(post::Command),
    Create(CreateAdjustmentBatch),
    Replace(Replacement),
}
impl Command {
    pub(super) fn parse(kind: &str, value: Value) -> Result<Self, StoreError> {
        let invalid = || StoreError::Invalid("adjustment draft input".into());
        match kind {
            "operational_adjustment_post_intent" => {
                Ok(Self::Post(post::Command::parse(kind, value)?))
            }
            "operational_adjustment_creation_intent" => Ok(Self::Create(
                serde_json::from_value(value).map_err(|_| invalid())?,
            )),
            "operational_adjustment_update_intent" => {
                let input: Replacement = serde_json::from_value(value).map_err(|_| invalid())?;
                if input.batch_id.is_nil() || input.expected_version < 1 {
                    return Err(invalid());
                }
                Ok(Self::Replace(input))
            }
            _ => Err(StoreError::NotFoundOrForbidden),
        }
    }
    pub(super) fn value(&self) -> Result<Value, StoreError> {
        match self {
            Self::Post(v) => v.value(),
            Self::Create(v) => serde_json::to_value(v)
                .map_err(|_| StoreError::Invalid("adjustment draft input".into())),
            Self::Replace(v) => serde_json::to_value(v)
                .map_err(|_| StoreError::Invalid("adjustment draft input".into())),
        }
    }
    pub(super) fn action(&self) -> &'static str {
        match self {
            Self::Post(v) => v.action(),
            Self::Create(_) => "profit_adjustment:create",
            Self::Replace(_) => "profit_adjustment:update_draft",
        }
    }
    pub(super) fn preview_permission(&self) -> &'static str {
        match self {
            Self::Post(_) => "profit_adjustment:preview",
            _ => self.action(),
        }
    }
    pub(super) fn result_field(&self) -> &'static str {
        match self {
            Self::Post(_) => "postedDocument",
            Self::Create(_) => "createdDocument",
            Self::Replace(_) => "updatedDocument",
        }
    }
    pub(super) async fn preview_on(
        &self,
        service: &AdjustmentService,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
    ) -> Result<Value, StoreError> {
        let (source, input) = match self {
            Self::Post(v) => return v.preview_on(service, tx, actor).await,
            Self::Create(v) => (None, v),
            Self::Replace(v) => (Some((v.batch_id, v.expected_version)), &v.batch),
        };
        let preview = service
            .draft_preview_on(tx, actor, source, input)
            .await
            .map_err(domain_error)?;
        Ok(
            json!({"schemaVersion":1,"kind":preview["preview"]["kind"],"input":self.value()?,"ownerUserId":actor,"scope":preview["preview"]["scope"],"draftPreview":preview,"effects":preview["preview"]["effects"],"boundary":"management_only_not_general_ledger"}),
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
        let (source, input) = match self {
            Self::Post(v) => return v.save_on(service, tx, context, request, snapshot).await,
            Self::Create(v) => (None, v),
            Self::Replace(v) => (Some((v.batch_id, v.expected_version)), &v.batch),
        };
        let result = service
            .apply_draft_preview_on(
                tx,
                context.0,
                context.1,
                &format!("chat-adjustment-{request}"),
                source,
                input,
                &snapshot["draftPreview"],
            )
            .await
            .map_err(domain_error)?;
        serde_json::to_value(result)
            .map_err(|_| StoreError::Invalid("adjustment draft result".into()))
    }
}
