//! Exact and bounded operational-adjustment reads before draft edits or posting.
use super::*;
/// Find visible batches by literal number and exact business filters.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchOperationalAdjustmentsInput {
    /// Literal number substring, not a SQL wildcard.
    pub number: Option<String>,
    /// Optional authorized legal entity.
    pub legal_entity_id: Option<Uuid>,
    /// Optional YYYY-MM management period.
    pub management_period: Option<String>,
    /// Draft, previewed, posted, reversed or cancelled.
    pub status: Option<String>,
    /// Last visible ID returned by the preceding page.
    pub after_id: Option<Uuid>,
    /// Maximum 20 batch summaries.
    #[serde(default = "search_limit")]
    pub limit: u32,
}
fn search_limit() -> u32 {
    20
}
/// Read every line page with the original version before editing an adjustment.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetOperationalAdjustmentInput {
    /// Exact batch UUID obtained from authorized lookup.
    pub document_id: Uuid,
    /// Required for pages after offset zero.
    pub expected_version: Option<i64>,
    /// Zero-based line offset.
    #[serde(default)]
    pub offset: u32,
    /// Maximum 10 full lines per response.
    #[serde(default = "detail_limit")]
    pub limit: u32,
}
fn detail_limit() -> u32 {
    10
}
impl ValidateInput for SearchOperationalAdjustmentsInput {
    fn validate_and_normalize(&mut self, _today: NaiveDate) -> Result<(), ValidationError> {
        if !(1..=20).contains(&self.limit) {
            return Err(ValidationError::LimitExceeded);
        }
        if self.number.as_deref().is_some_and(|s| {
            s.trim().is_empty() || s.chars().count() > 80 || s.chars().any(char::is_control)
        }) || self.status.as_deref().is_some_and(|s| {
            !matches!(
                s,
                "draft" | "previewed" | "posted" | "reversed" | "cancelled"
            )
        }) || self.management_period.as_deref().is_some_and(|s| {
            s.len() != 7 || NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d").is_err()
        }) {
            return Err(ValidationError::UnsafeText);
        }
        if self.after_id.is_some_and(|id| id.is_nil())
            || self.legal_entity_id.is_some_and(|id| id.is_nil())
        {
            return Err(ValidationError::InvalidCursor);
        }
        Ok(())
    }
}
impl ValidateInput for GetOperationalAdjustmentInput {
    fn validate_and_normalize(&mut self, _today: NaiveDate) -> Result<(), ValidationError> {
        if !(1..=10).contains(&self.limit) {
            return Err(ValidationError::LimitExceeded);
        }
        if self.document_id.is_nil()
            || self.offset > 10000
            || (self.offset > 0 && self.expected_version.is_none())
            || self.expected_version.is_some_and(|v| v < 1)
        {
            return Err(ValidationError::InvalidCursor);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_adjustment_filters_and_version_bound_pages() {
        let today = Utc::now().date_naive();
        for v in [
            serde_json::json!({"limit":21}),
            serde_json::json!({"managementPeriod":"2026-13"}),
            serde_json::json!({"status":"anything"}),
        ] {
            let mut input: SearchOperationalAdjustmentsInput = serde_json::from_value(v).unwrap();
            assert!(input.validate_and_normalize(today).is_err());
        }
        let mut literal: SearchOperationalAdjustmentsInput =
            serde_json::from_value(serde_json::json!({"number":"ADJ-%"})).unwrap();
        assert!(literal.validate_and_normalize(today).is_ok());
        let mut detail: GetOperationalAdjustmentInput =
            serde_json::from_value(serde_json::json!({"documentId":Uuid::new_v4(),"offset":1}))
                .unwrap();
        assert!(detail.validate_and_normalize(today).is_err());
        detail.expected_version = Some(1);
        assert!(detail.validate_and_normalize(today).is_ok());
        assert!(serde_json::from_value::<GetOperationalAdjustmentInput>(
            serde_json::json!({"documentId":Uuid::new_v4(),"sql":"SELECT *"})
        )
        .is_err());
    }
}
