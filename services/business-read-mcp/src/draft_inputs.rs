use super::*;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SalesDraftReplacement {
    expected_version: i64,
    customer_id: Uuid,
    business_unit_id: Uuid,
    #[serde(default)]
    department_id: Option<Uuid>,
    #[serde(default)]
    brand_id: Option<Uuid>,
    currency: String,
    order_date: String,
    #[serde(default)]
    requested_delivery_date: Option<String>,
    #[serde(default)]
    payment_terms_days: Option<i32>,
    #[serde(default)]
    customer_reference: Option<String>,
    #[serde(default)]
    business_note: Option<String>,
    lines: Vec<OrderLineInput>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PurchaseDraftReplacement {
    expected_version: i64,
    legal_entity_id: Uuid,
    supplier_id: Uuid,
    #[serde(default)]
    buyer_user_id: Option<Uuid>,
    business_unit_id: Uuid,
    #[serde(default)]
    department_id: Option<Uuid>,
    #[serde(default)]
    brand_id: Option<Uuid>,
    currency: String,
    order_date: String,
    #[serde(default)]
    expected_delivery_date: Option<String>,
    #[serde(default)]
    payment_terms_days: Option<i32>,
    #[serde(default)]
    supplier_reference: Option<String>,
    #[serde(default)]
    business_note: Option<String>,
    lines: Vec<OrderLineInput>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct UpdateSalesOrderDraftInput {
    document_id: Uuid,
    draft: SalesDraftReplacement,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct UpdatePurchaseOrderDraftInput {
    document_id: Uuid,
    draft: PurchaseDraftReplacement,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct OpeningLineInput {
    warehouse_id: Uuid,
    sku_id: Uuid,
    quantity: String,
    unit_cost: String,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CreateInventoryOpeningDraftInput {
    legal_entity_id: Uuid,
    business_date: String,
    currency: String,
    lines: Vec<OpeningLineInput>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PrepareAllocationInput {
    source_document_id: Uuid,
    expected_source_version: i64,
    allocations: Vec<AllocationTargetInput>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AllocationTargetInput {
    document_id: Uuid,
    expected_version: i64,
    amount: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PrepareReversalInput {
    source_document_id: Uuid,
    expected_source_version: i64,
    allocation_id: Option<Uuid>,
    expected_target_version: Option<i64>,
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PrepareOrderCancellationInput {
    source_document_id: Uuid,
    expected_source_version: i64,
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PrepareStockReversalInput {
    /// ID of the selected fulfillment document from an authorized read.
    source_document_id: Uuid,
    /// Current source version, never guessed.
    expected_source_version: i64,
    /// User-provided reason, at most 500 characters.
    reason: String,
}
