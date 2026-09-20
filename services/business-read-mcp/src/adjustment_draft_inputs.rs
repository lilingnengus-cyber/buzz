//! Strict model-facing draft inputs; amounts and weights never use floating point.
use super::*;
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CreateAdjustmentBatch {
    pub legal_entity_id: Uuid,
    pub currency: String,
    /// YYYY-MM management period.
    pub management_period: String,
    /// Complete draft lines, including all preserved lines when replacing.
    pub lines: Vec<AdjustmentLine>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReplaceAdjustmentInput {
    pub batch_id: Uuid,
    pub expected_version: i64,
    pub batch: CreateAdjustmentBatch,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AdjustmentLine {
    pub metric_type: Metric,
    /// Positive decimal string with at most two nonzero decimal places.
    pub amount: String,
    /// YYYY-MM-DD business date.
    pub business_date: String,
    pub allocation_basis: Basis,
    #[serde(default)]
    pub direct_sales_order_id: Option<Uuid>,
    #[serde(default)]
    pub customer_id: Option<Uuid>,
    #[serde(default)]
    pub sku_id: Option<Uuid>,
    #[serde(default)]
    pub brand_id: Option<Uuid>,
    #[serde(default)]
    pub salesperson_user_id: Option<Uuid>,
    #[serde(default)]
    pub business_unit_id: Option<Uuid>,
    #[serde(default)]
    pub department_id: Option<Uuid>,
    #[serde(default)]
    pub warehouse_id: Option<Uuid>,
    #[serde(default)]
    pub sales_order_ids: Vec<Uuid>,
    #[serde(default)]
    pub fixed_weights: Vec<Weight>,
    pub reason_code: String,
    #[serde(default)]
    pub source_reference: Option<String>,
    #[serde(default)]
    pub business_note: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Weight {
    pub sales_order_id: Uuid,
    pub weight: String,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum Metric {
    OutboundFreight,
    SalesCommission,
    PlatformFee,
    CustomerRebate,
    SupplierRebate,
    OtherDirectCost,
    AllocatedOperatingExpense,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum Basis {
    Direct,
    NetRevenue,
    ProductCost,
    ShippedQuantity,
    FixedWeight,
}
