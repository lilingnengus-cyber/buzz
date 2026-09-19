//! Strict, bounded lookup contracts for inventory-count selection.
use super::*;

/// Locate count documents within the caller's current and frozen data scopes.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchInventoryCountsInput {
    /// Exact count UUID.
    pub document_id: Option<Uuid>,
    /// Literal substring of the count number.
    pub query: Option<String>,
    /// Exact legal entity.
    pub legal_entity_id: Option<Uuid>,
    /// Exact warehouse.
    pub warehouse_id: Option<Uuid>,
    /// Only counts containing this SKU.
    pub sku_id: Option<Uuid>,
    /// One of counting, counted, posted, cancelled.
    pub status: Option<String>,
    /// Offset in the source result set.
    #[serde(default)]
    pub offset: u32,
    /// Maximum page size, 1–100.
    #[serde(default = "limit")]
    pub limit: u32,
}

/// Locate available stock balances before preparing a count that freezes stock.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchInventoryCountOptionsInput {
    /// Literal substring of SKU code or name.
    pub query: Option<String>,
    /// Exact legal entity.
    pub legal_entity_id: Option<Uuid>,
    /// Exact warehouse.
    pub warehouse_id: Option<Uuid>,
    /// Exact SKU.
    pub sku_id: Option<Uuid>,
    /// Offset in the source result set.
    #[serde(default)]
    pub offset: u32,
    /// Maximum page size, 1–100.
    #[serde(default = "limit")]
    pub limit: u32,
}
fn limit() -> u32 {
    DEFAULT_LIMIT
}
fn page(offset: u32, limit: u32) -> Result<(), ValidationError> {
    if offset > 100_000 {
        return Err(ValidationError::InvalidCursor);
    }
    if limit == 0 || limit > MAX_LIMIT {
        return Err(ValidationError::LimitExceeded);
    }
    Ok(())
}
impl ValidateInput for SearchInventoryCountsInput {
    fn validate_and_normalize(&mut self, _today: NaiveDate) -> Result<(), ValidationError> {
        normalize_optional(&mut self.query)?;
        normalize_optional(&mut self.status)?;
        if self
            .status
            .as_deref()
            .is_some_and(|s| !matches!(s, "counting" | "counted" | "posted" | "cancelled"))
        {
            return Err(ValidationError::UnsafeText);
        }
        page(self.offset, self.limit)
    }
}
impl ValidateInput for SearchInventoryCountOptionsInput {
    fn validate_and_normalize(&mut self, _today: NaiveDate) -> Result<(), ValidationError> {
        normalize_optional(&mut self.query)?;
        page(self.offset, self.limit)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filters_are_strict_and_bounded() {
        for value in [
            json!({"warehouseId":"guess"}),
            json!({"partyId":Uuid::new_v4()}),
            json!({"execute":true}),
            json!({"offset":-1}),
        ] {
            assert!(serde_json::from_value::<SearchInventoryCountsInput>(value).is_err());
        }
        for value in [
            json!({"status":"draft"}),
            json!({"limit":0}),
            json!({"limit":101}),
            json!({"offset":100001}),
            json!({"query":"%"}),
        ] {
            let mut input: SearchInventoryCountsInput = serde_json::from_value(value).unwrap();
            assert!(input
                .validate_and_normalize(Utc::now().date_naive())
                .is_err());
        }
        let mut input: SearchInventoryCountOptionsInput =
            serde_json::from_value(json!({"query":"  商品  ","limit":100})).unwrap();
        input
            .validate_and_normalize(Utc::now().date_naive())
            .unwrap();
        assert_eq!(input.query.as_deref(), Some("商品"));
        assert!(serde_json::from_value::<SearchInventoryCountOptionsInput>(
            json!({"status":"posted"})
        )
        .is_err());
    }
    use serde_json::json;
}
