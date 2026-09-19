use super::*;

/// Exact identifier obtained from a scoped business document search.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetBusinessDocumentInput {
    /// Authoritative business document UUID.
    pub document_id: Uuid,
}
impl ValidateInput for GetBusinessDocumentInput {
    fn validate_and_normalize(&mut self, _today: NaiveDate) -> Result<(), ValidationError> {
        if self.document_id.is_nil() {
            return Err(ValidationError::UnsafeText);
        }
        Ok(())
    }
}
