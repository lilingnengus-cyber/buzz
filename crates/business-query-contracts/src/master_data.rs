//! Bounded, read-only lookup of records used by draft-entry tools.
use super::*;

/// Existing capability required for reading business master records.
pub const MASTER_DATA_READ: &str = "business_master_data:read";

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
/// Fixed catalog categories available to draft-entry agents.
pub enum MasterDataKind {
    LegalEntity,
    Customer,
    Supplier,
    BusinessUnit,
    Sku,
    Warehouse,
    UnitOfMeasure,
    Brand,
    Product,
    ProductCategory,
    UomConversion,
}

impl MasterDataKind {
    /// Returns the fixed Core resource path segment.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LegalEntity => "legal_entity",
            Self::Customer => "customer",
            Self::Supplier => "supplier",
            Self::BusinessUnit => "business_unit",
            Self::Sku => "sku",
            Self::Warehouse => "warehouse",
            Self::UnitOfMeasure => "unit_of_measure",
            Self::Brand => "brand",
            Self::Product => "product",
            Self::ProductCategory => "product_category",
            Self::UomConversion => "uom_conversion",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Name/code lookup with optional entity narrowing and bounded pagination.
pub struct SearchMasterDataInput {
    /// Catalog category to search.
    pub resource_type: MasterDataKind,
    #[serde(default)]
    /// Literal case-insensitive substring of a name or code.
    pub query: Option<String>,
    #[serde(default)]
    /// Optional legal entity filter; never broadens the current scope.
    pub legal_entity_id: Option<Uuid>,
    #[serde(default)]
    /// Offset in the authorized source result set.
    pub offset: u32,
    #[serde(default = "default_master_limit")]
    /// Maximum number of source records to consider on this page.
    pub limit: u32,
}
fn default_master_limit() -> u32 {
    20
}
impl ValidateInput for SearchMasterDataInput {
    fn validate_and_normalize(&mut self, _today: NaiveDate) -> Result<(), ValidationError> {
        // Names are literal data, not identifiers: conversion labels contain '/',
        // and real business names may contain parentheses or wildcard characters.
        // Core uses bound strpos operands, so these never become SQL patterns.
        if let Some(query) = &mut self.query {
            let trimmed = query.trim();
            if trimmed.is_empty()
                || trimmed.chars().count() > MAX_TEXT_CHARS
                || trimmed.chars().any(char::is_control)
            {
                return Err(ValidationError::UnsafeText);
            }
            *query = trimmed.to_owned();
        }
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
    fn master_links_have_a_closed_kind_and_uuid_path() {
        let id = Uuid::new_v4();
        for kind in [
            "legal_entity",
            "business_unit",
            "customer",
            "supplier",
            "warehouse",
            "unit_of_measure",
            "product_category",
            "brand",
            "product",
            "sku",
            "uom_conversion",
        ] {
            assert!(valid_biz_uri(&format!("biz://master-data/{kind}/{id}")));
        }
        for path in [
            format!("users/{id}"),
            "product/not-a-uuid".into(),
            format!("product/{id}/extra"),
            format!("product/{id}?token=bad"),
        ] {
            assert!(!valid_biz_uri(&format!("biz://master-data/{path}")));
        }
    }
    #[test]
    fn names_are_bounded_literals_including_conversion_labels() {
        for query in ["产品 / 箱", "客户（杭州）", "%", "O'Reilly", "_", "a\nb"] {
            let mut input: SearchMasterDataInput =
                serde_json::from_value(serde_json::json!({"resourceType":"product","query":query}))
                    .unwrap();
            assert_eq!(
                input
                    .validate_and_normalize(Utc::now().date_naive())
                    .is_ok(),
                !query.contains('\n')
            );
        }
    }
    #[test]
    fn product_catalog_kinds_have_fixed_core_paths() {
        for kind in ["product", "product_category", "uom_conversion"] {
            let input: SearchMasterDataInput =
                serde_json::from_value(serde_json::json!({"resourceType":kind})).unwrap();
            assert_eq!(input.resource_type.as_str(), kind);
        }
    }
    #[test]
    fn rejects_arbitrary_resources_and_unbounded_searches() {
        assert!(serde_json::from_value::<SearchMasterDataInput>(
            serde_json::json!({"resourceType":"users"})
        )
        .is_err());
        assert!(serde_json::from_value::<SearchMasterDataInput>(
            serde_json::json!({"resourceType":"customer","url":"https://invalid"})
        )
        .is_err());
        let mut input: SearchMasterDataInput = serde_json::from_value(
            serde_json::json!({"resourceType":"customer","query":"  客户  "}),
        )
        .unwrap();
        input
            .validate_and_normalize(Utc::now().date_naive())
            .unwrap();
        assert_eq!(input.query.as_deref(), Some("客户"));
        input.limit = 101;
        assert!(input
            .validate_and_normalize(Utc::now().date_naive())
            .is_err());
        input.limit = 20;
        input.offset = 100_001;
        assert!(input
            .validate_and_normalize(Utc::now().date_naive())
            .is_err());
    }
}
