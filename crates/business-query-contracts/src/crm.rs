//! Bounded CRM lookups used before preparing exact write commands.
use super::*;

/// Find opportunities by literal name, exact identifiers, stage and due date.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchCrmOpportunitiesInput {
    /// Exact opportunity UUID.
    pub document_id: Option<Uuid>,
    /// Literal title, company or contact substring.
    pub query: Option<String>,
    /// Restrict to this legal entity.
    pub legal_entity_id: Option<Uuid>,
    /// Restrict to this business unit.
    pub business_unit_id: Option<Uuid>,
    /// Restrict to this existing customer.
    pub customer_id: Option<Uuid>,
    /// One of new, contacting, quoting, won or lost.
    pub stage: Option<String>,
    /// Open opportunities due on or before this date.
    pub due_by: Option<NaiveDate>,
    /// Source offset; continue even if an intermediate filtered page is empty.
    #[serde(default)]
    pub offset: u32,
    /// At most 20 summaries per page.
    #[serde(default = "search_limit")]
    pub limit: u32,
}
fn search_limit() -> u32 {
    20
}
/// Read an exact opportunity and a version-bound page of follow-ups.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetCrmOpportunityInput {
    /// Exact opportunity UUID.
    pub document_id: Uuid,
    /// Required after the first page, using the first page's version.
    pub expected_version: Option<i64>,
    /// Offset in the immutable follow-up history.
    #[serde(default)]
    pub offset: u32,
    /// At most three full notes to stay within the agent response budget.
    #[serde(default = "detail_limit")]
    pub limit: u32,
}
fn detail_limit() -> u32 {
    3
}
impl ValidateInput for SearchCrmOpportunitiesInput {
    fn validate_and_normalize(&mut self, _today: NaiveDate) -> Result<(), ValidationError> {
        normalize_optional(&mut self.query)?;
        normalize_optional(&mut self.stage)?;
        if self
            .stage
            .as_deref()
            .is_some_and(|s| !matches!(s, "new" | "contacting" | "quoting" | "won" | "lost"))
        {
            return Err(ValidationError::UnsafeText);
        }
        if self.offset > 100000 {
            return Err(ValidationError::InvalidCursor);
        }
        if !(1..=20).contains(&self.limit) {
            return Err(ValidationError::LimitExceeded);
        }
        Ok(())
    }
}
impl ValidateInput for GetCrmOpportunityInput {
    fn validate_and_normalize(&mut self, _today: NaiveDate) -> Result<(), ValidationError> {
        if self.offset > 100000
            || (self.offset > 0 && self.expected_version.is_none())
            || self.expected_version.is_some_and(|v| v < 1)
        {
            return Err(ValidationError::InvalidCursor);
        }
        if !(1..=3).contains(&self.limit) {
            return Err(ValidationError::LimitExceeded);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn strict_filters_and_version_bound_pages() {
        let today = Utc::now().date_naive();
        for value in [
            json!({"limit":21}),
            json!({"stage":"unknown"}),
            json!({"offset":100001}),
            json!({"query":"%"}),
        ] {
            let mut input: SearchCrmOpportunitiesInput = serde_json::from_value(value).unwrap();
            assert!(input.validate_and_normalize(today).is_err());
        }
        assert!(
            serde_json::from_value::<SearchCrmOpportunitiesInput>(json!({"execute":true})).is_err()
        );
        let id = Uuid::new_v4();
        for value in [
            json!({"documentId":id,"offset":3}),
            json!({"documentId":id,"limit":4}),
            json!({"documentId":id,"expectedVersion":0}),
        ] {
            let mut input: GetCrmOpportunityInput = serde_json::from_value(value).unwrap();
            assert!(input.validate_and_normalize(today).is_err());
        }
        let mut good: SearchCrmOpportunitiesInput =
            serde_json::from_value(json!({"query":"  商机  "})).unwrap();
        good.validate_and_normalize(today).unwrap();
        assert_eq!(good.query.as_deref(), Some("商机"));
    }
}
