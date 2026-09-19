//! Bounded financial document lookup before preparing a business mutation.
use super::*;

/// Search one fixed document family by ID, number, party, or lifecycle status.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchBusinessDocumentsInput {
    /// Exact document ID; filters are combined rather than overriding authorization.
    pub document_id: Option<Uuid>,
    /// Literal case-insensitive substring of the document number.
    pub query: Option<String>,
    /// Customer or supplier ID resolved through authorized master-data lookup.
    pub party_id: Option<Uuid>,
    /// Exact lifecycle status, such as draft, confirmed, posted, reversed, or open.
    pub status: Option<String>,
    /// Offset in the authorized source result set.
    #[serde(default)]
    pub offset: u32,
    /// Maximum records on one page.
    #[serde(default = "default_limit")]
    pub limit: u32,
}
/// Bounded lookup of financial sources and allocation targets.
pub type SearchFinancialDocumentsInput = SearchBusinessDocumentsInput;
/// Bounded lookup of shipment, goods receipt or inventory opening sources.
pub type SearchStockDocumentsInput = SearchBusinessDocumentsInput;
impl ValidateInput for SearchBusinessDocumentsInput {
    fn validate_and_normalize(&mut self, _today: NaiveDate) -> Result<(), ValidationError> {
        normalize_optional(&mut self.query)?;
        normalize_optional(&mut self.status)?;
        if self.offset > 100_000 {
            return Err(ValidationError::InvalidCursor);
        }
        if self.limit == 0 || self.limit > MAX_LIMIT {
            return Err(ValidationError::LimitExceeded);
        }
        Ok(())
    }
}

fn default_limit() -> u32 {
    DEFAULT_LIMIT
}

/// Read the allocation history of one explicitly identified receipt or payment.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettlementAllocationsInput {
    /// Current authorized source receipt/payment ID.
    pub source_document_id: Uuid,
    /// Offset in source allocation history, including records hidden by narrower target scope.
    #[serde(default)]
    pub offset: u32,
    /// Maximum source allocation records examined per page.
    #[serde(default = "default_limit")]
    pub limit: u32,
}
impl ValidateInput for SettlementAllocationsInput {
    fn validate_and_normalize(&mut self, _today: NaiveDate) -> Result<(), ValidationError> {
        if self.offset > 100_000 {
            return Err(ValidationError::InvalidCursor);
        }
        if self.limit == 0 || self.limit > MAX_LIMIT {
            return Err(ValidationError::LimitExceeded);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lookup_rejects_unbounded_or_untyped_filters() {
        for value in [
            serde_json::json!({"documentId":"guess"}),
            serde_json::json!({"url":"http://example.invalid"}),
            serde_json::json!({"offset":-1}),
        ] {
            assert!(serde_json::from_value::<SearchFinancialDocumentsInput>(value).is_err());
        }
        for value in [
            serde_json::json!({"limit":0}),
            serde_json::json!({"limit":101}),
            serde_json::json!({"offset":100001}),
            serde_json::json!({"query":"%"}),
        ] {
            let mut input: SearchFinancialDocumentsInput = serde_json::from_value(value).unwrap();
            assert!(input
                .validate_and_normalize(Utc::now().date_naive())
                .is_err());
        }
        let mut input: SearchFinancialDocumentsInput =
            serde_json::from_value(serde_json::json!({"query":"  RC-202609  "})).unwrap();
        input
            .validate_and_normalize(Utc::now().date_naive())
            .unwrap();
        assert_eq!(input.query.as_deref(), Some("RC-202609"));
    }
}
