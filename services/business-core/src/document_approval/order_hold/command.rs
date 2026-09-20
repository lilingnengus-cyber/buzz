use super::*;
use crate::b2::{model::VersionCommand, SalesService};
use sqlx::{Postgres, Transaction};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    source_document_id: Uuid,
    expected_source_version: i64,
    reason: String,
}
pub(super) struct Command {
    input: Input,
    place: bool,
}
impl Command {
    pub(super) fn parse(kind: &str, value: Value) -> Result<Self, StoreError> {
        let place = match kind {
            "sales_order_hold_intent" => true,
            "sales_order_release_hold_intent" => false,
            _ => return Err(StoreError::NotFoundOrForbidden),
        };
        let mut input: Input = serde_json::from_value(value)
            .map_err(|_| StoreError::Invalid("order hold input".into()))?;
        input.reason = input.reason.trim().to_owned();
        if input.expected_source_version <= 0 || input.reason.is_empty() || input.reason.len() > 64
        {
            return Err(StoreError::Invalid(
                "positive version and reason of at most 64 bytes required".into(),
            ));
        }
        Ok(Self { input, place })
    }
    pub(super) fn value(&self) -> Result<Value, StoreError> {
        serde_json::to_value(&self.input)
            .map_err(|_| StoreError::Invalid("order hold input".into()))
    }
    pub(super) fn action(&self) -> &'static str {
        if self.place {
            "sales_order:place_hold"
        } else {
            "sales_order:release_hold"
        }
    }
    fn version(&self) -> VersionCommand {
        VersionCommand {
            expected_version: self.input.expected_source_version,
            reason_code: Some(self.input.reason.clone()),
        }
    }
    pub(super) async fn preview_on(
        &self,
        sales: &SalesService,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
    ) -> Result<Value, StoreError> {
        sales
            .hold_preview_on(
                tx,
                actor,
                self.input.source_document_id,
                &self.version(),
                self.place,
            )
            .await
            .map_err(domain_error)
    }
    pub(super) async fn save_on(
        &self,
        sales: &SalesService,
        tx: &mut Transaction<'_, Postgres>,
        context: (Uuid, Uuid),
        request: Uuid,
        snapshot: &Value,
    ) -> Result<Value, StoreError> {
        let result = sales
            .set_hold_on(
                tx,
                context,
                (self.input.source_document_id, self.place),
                &format!("chat-order-hold-{request}"),
                &self.version(),
                Some(snapshot),
            )
            .await
            .map_err(domain_error)?;
        serde_json::to_value(result).map_err(|_| StoreError::Invalid("order hold result".into()))
    }
}
fn domain_error(e: crate::b2::DomainError) -> StoreError {
    match e {
        crate::b2::DomainError::NotFoundOrForbidden => StoreError::NotFoundOrForbidden,
        crate::b2::DomainError::VersionConflict
        | crate::b2::DomainError::StalePreview
        | crate::b2::DomainError::IdempotencyConflict => StoreError::Conflict,
        crate::b2::DomainError::Database(e) => StoreError::Database(e),
        _ => StoreError::Invalid("order hold input or state".into()),
    }
}
