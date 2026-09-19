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

/// A hash-bound page of an existing inventory-count approval intent.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetInventoryCountPreviewInput {
    /// Exact immutable approval intent UUID, not the inventory count UUID.
    pub document_id: Uuid,
    /// Fixed inventory-count intent family.
    pub document_type: String,
    /// Full server snapshot hash returned by preparation.
    pub preview_hash: String,
    /// Zero-based line offset within that same snapshot.
    #[serde(default)]
    pub offset: u32,
    /// Up to 20 complete lines per page.
    #[serde(default = "preview_limit")]
    pub limit: u32,
}
fn preview_limit() -> u32 {
    20
}
impl ValidateInput for GetInventoryCountPreviewInput {
    fn validate_and_normalize(&mut self, _today: NaiveDate) -> Result<(), ValidationError> {
        if !matches!(
            self.document_type.as_str(),
            "inventory_count_creation_intent"
                | "inventory_count_submission_intent"
                | "inventory_count_posting_intent"
                | "inventory_count_cancellation_intent"
        ) || self.preview_hash.len() != 64
            || !self.preview_hash.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(ValidationError::UnsafeText);
        }
        if self.offset > 500 {
            return Err(ValidationError::InvalidCursor);
        }
        if self.limit == 0 || self.limit > 20 {
            return Err(ValidationError::LimitExceeded);
        }
        Ok(())
    }
}

/// Read one version-bound page of inventory count lines.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetInventoryCountInput {
    /// Exact inventory count UUID.
    pub document_id: Uuid,
    /// Version from the first page; mandatory after offset zero.
    pub expected_version: Option<i64>,
    /// Zero-based line offset.
    #[serde(default)]
    pub offset: u32,
    /// Up to 20 lines.
    #[serde(default = "preview_limit")]
    pub limit: u32,
}
impl ValidateInput for GetInventoryCountInput {
    fn validate_and_normalize(&mut self, _today: NaiveDate) -> Result<(), ValidationError> {
        if self.offset > 500
            || (self.offset > 0 && self.expected_version.is_none())
            || self.expected_version.is_some_and(|v| v < 1)
        {
            return Err(ValidationError::InvalidCursor);
        }
        if self.limit == 0 || self.limit > 20 {
            return Err(ValidationError::LimitExceeded);
        }
        Ok(())
    }
}

#[cfg(test)]
mod page_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn pages_require_bounded_offsets_and_fixed_snapshot_identity() {
        let id = Uuid::new_v4();
        let original = json!({"documentId":id,"documentType":"inventory_count_submission_intent","previewHash":"a".repeat(64)});
        let mut valid: GetInventoryCountPreviewInput =
            serde_json::from_value(original.clone()).unwrap();
        assert!(valid
            .validate_and_normalize(Utc::now().date_naive())
            .is_ok());
        for (key, value) in [
            ("documentType", json!("sales_order")),
            ("previewHash", json!("short")),
            ("offset", json!(501)),
            ("limit", json!(21)),
            ("limit", json!(0)),
        ] {
            let mut bad = original.clone();
            bad[key] = value;
            let mut bad: GetInventoryCountPreviewInput = serde_json::from_value(bad).unwrap();
            assert!(bad.validate_and_normalize(Utc::now().date_naive()).is_err());
        }
        let mut extra = original;
        extra["execute"] = json!(true);
        assert!(serde_json::from_value::<GetInventoryCountPreviewInput>(extra).is_err());
        for value in [
            json!({"documentId":id,"offset":20}),
            json!({"documentId":id,"expectedVersion":0}),
            json!({"documentId":id,"limit":21}),
        ] {
            let mut input: GetInventoryCountInput = serde_json::from_value(value).unwrap();
            assert!(input
                .validate_and_normalize(Utc::now().date_naive())
                .is_err());
        }
        let mut valid: GetInventoryCountInput =
            serde_json::from_value(json!({"documentId":id,"offset":20,"expectedVersion":1}))
                .unwrap();
        assert!(valid
            .validate_and_normalize(Utc::now().date_naive())
            .is_ok());
    }
}
