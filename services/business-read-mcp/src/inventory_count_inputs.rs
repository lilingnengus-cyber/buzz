use super::*;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CountCreationInput {
    legal_entity_id: Uuid,
    warehouse_id: Uuid,
    /// Explicit local business date in YYYY-MM-DD form.
    count_date: String,
    currency: String,
    business_note: Option<String>,
    /// One to 500 unique IDs selected from authorized count options.
    sku_ids: Vec<Uuid>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CountLineInput {
    count_line_id: Uuid,
    /// Actual total physical on-hand quantity, encoded as an exact decimal string.
    actual_on_hand_quantity: String,
    /// Supply only when a surplus requires a human-provided unit cost.
    surplus_unit_cost: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CountSubmission {
    expected_version: i64,
    /// Every count line exactly once; never infer missing physical counts.
    lines: Vec<CountLineInput>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CountPosting {
    expected_version: i64,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CountCancellation {
    expected_version: i64,
    reason_code: String,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CountOperationInput<T> {
    inventory_count_id: Uuid,
    command: T,
}
