use super::*;
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CrmFields {
    /// Exact authorized legal entity and business unit; do not guess.
    legal_entity_id: Uuid,
    business_unit_id: Uuid,
    /// Null for an unlinked prospect; otherwise an exact existing customer ID.
    customer_id: Option<Uuid>,
    title: String,
    company_name: String,
    #[serde(default)]
    contact_name: String,
    #[serde(default)]
    contact_details: String,
    /// new, contacting, quoting, won or lost; won does not create a sales order.
    stage: String,
    /// Integer minor currency units, e.g. CNY 20000 means 200 yuan. Never guess.
    expected_amount_minor: Option<i64>,
    currency: String,
    #[serde(default)]
    next_action: String,
    /// Explicit local YYYY-MM-DD, or null if no date was given.
    next_follow_up: Option<String>,
    /// Omit for creation. Required current version for replacing an existing opportunity.
    expected_version: Option<i64>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CrmFollowup {
    /// Human-provided follow-up facts; not inferred actions or invented contact history.
    note: String,
    stage: String,
    next_action: String,
    next_follow_up: Option<String>,
    expected_version: i64,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CrmTarget<T> {
    opportunity_id: Uuid,
    command: T,
}
