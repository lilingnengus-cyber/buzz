use super::*;
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum CoreKind {
    LegalEntity,
    BusinessUnit,
    Customer,
    Supplier,
    Warehouse,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum ProductKind {
    UnitOfMeasure,
    ProductCategory,
    Brand,
    Product,
    Sku,
    UomConversion,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum MasterKind {
    LegalEntity,
    BusinessUnit,
    Customer,
    Supplier,
    Warehouse,
    UnitOfMeasure,
    ProductCategory,
    Brand,
    Product,
    Sku,
    UomConversion,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CoreCreation {
    resource_type: CoreKind,
    /// Human-approved unique uppercase code. Ask if absent; do not invent identifiers.
    code: String,
    name: String,
    legal_entity_id: Option<Uuid>,
    business_unit_id: Option<Uuid>,
    country_code: Option<String>,
    functional_currency: Option<String>,
    registration_number: Option<String>,
    address: Option<String>,
    credit_currency: Option<String>,
    /// Exact integer minor currency units. Display the currency and normal amount in the preview.
    credit_limit_minor: Option<i64>,
    payment_terms_days: Option<i32>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ProductCreation {
    resource_type: ProductKind,
    /// Unique uppercase code; use empty string only for a unit conversion whose code is derived.
    code: String,
    /// Use empty string only for a unit conversion whose name is derived.
    name: String,
    parent_category_id: Option<Uuid>,
    category_id: Option<Uuid>,
    brand_id: Option<Uuid>,
    base_uom_id: Option<Uuid>,
    product_id: Option<Uuid>,
    unit_of_measure_id: Option<Uuid>,
    barcode: Option<String>,
    /// Required for unit_of_measure: integer 0 through 6. Ask the human when absent.
    precision_scale: Option<i16>,
    allow_zero_cost: Option<bool>,
    /// Exact positive decimal string, at most eight fractional digits; never a floating-point number.
    factor_to_base: Option<String>,
    /// purchase, sales or both, for unit conversions only.
    usage_scope: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct MasterRecordInput {
    resource_type: MasterKind,
    document_id: Uuid,
}
impl ValidateInput for MasterRecordInput {
    fn validate_and_normalize(
        &mut self,
        _today: chrono::NaiveDate,
    ) -> Result<(), business_query_contracts::ValidationError> {
        Ok(())
    }
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct MasterPatch<K> {
    resource_type: K,
    document_id: Uuid,
    expected_version: i64,
    /// Only human-requested edits. Omit unchanged fields. Explicit null clears registrationNumber, address or barcode only.
    changes: MasterChanges,
}
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct MasterChanges {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    name: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    country_code: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    functional_currency: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    registration_number: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    address: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    credit_currency: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    credit_limit_minor: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    payment_terms_days: Option<i32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    allow_zero_cost: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    barcode: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    factor_to_base: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    usage_scope: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_preserve_omission_and_explicit_nullable_clear() {
        let input = json!({"resourceType":"warehouse","documentId":Uuid::new_v4(),
            "expectedVersion":2,"changes":{"name":"新仓库","address":null}});
        let patch: MasterPatch<CoreKind> = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(serde_json::to_value(patch).unwrap(), input);
        for changes in [
            json!({"name":null}),
            json!({"code":"REPLACED"}),
            json!({"status":"disabled"}),
            json!({"paymentTermsDays":1.5}),
            json!({"address":{"secret":"value"}}),
        ] {
            let mut invalid = input.clone();
            invalid["changes"] = changes;
            assert!(serde_json::from_value::<MasterPatch<CoreKind>>(invalid).is_err());
        }
    }

    #[test]
    fn resource_families_and_identifiers_are_closed() {
        assert!(serde_json::from_value::<ProductRecordInput>(
            json!({"resourceType":"customer","documentId":Uuid::new_v4()})
        )
        .is_err());
        assert!(serde_json::from_value::<CoreCreation>(json!({
            "resourceType":"sku","code":"SKU","name":"SKU"}))
        .is_err());
        assert!(serde_json::from_value::<MasterRecordInput>(json!({
            "resourceType":"warehouse","documentId":"invented"}))
        .is_err());
        let schema = serde_json::to_value(schemars::schema_for!(MasterPatch<CoreKind>)).unwrap();
        assert_eq!(schema["additionalProperties"], false);
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ProductRecordInput {
    resource_type: ProductKind,
    document_id: Uuid,
}
impl ValidateInput for ProductRecordInput {
    fn validate_and_normalize(
        &mut self,
        _today: chrono::NaiveDate,
    ) -> Result<(), business_query_contracts::ValidationError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum MasterStatus {
    Active,
    Disabled,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct MasterStatusChange<K> {
    resource_type: K,
    document_id: Uuid,
    #[schemars(range(min = 1))]
    expected_version: i64,
    status: MasterStatus,
}
