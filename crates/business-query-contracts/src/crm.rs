//! Fixed CRM lead tools; qualification reuses the versioned Business Core workflow.
use super::{ValidateInput, ValidationError};
use chrono::NaiveDate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Bounded, owner/creator-scoped lead search.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchCrmLeadsInput {
    /// Query.
    pub query: Option<String>,
    pub status: Option<String>,
    /// Owner user id.
    pub owner_user_id: Option<Uuid>,
    /// Exclusive local date cutoff for overdue open leads, YYYY-MM-DD.
    pub due_by: Option<String>,
    #[serde(default)]
    /// Offset.
    pub offset: i64,
}
impl ValidateInput for SearchCrmLeadsInput {
    fn validate_and_normalize(&mut self, _: NaiveDate) -> Result<(), ValidationError> {
        if !(0..=100000).contains(&self.offset)
            || self.query.as_ref().is_some_and(|s| s.chars().count() > 160)
            || self
                .status
                .as_deref()
                .is_some_and(|s| !["new", "contacting", "converted", "disqualified"].contains(&s))
            || self
                .due_by
                .as_ref()
                .is_some_and(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").is_err())
        {
            return Err(ValidationError::MissingContext);
        }
        Ok(())
    }
}
/// Resolve one lead using an ID returned by current authorized tools.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetCrmLeadInput {
    /// Lead id.
    pub lead_id: Uuid,
}
impl ValidateInput for GetCrmLeadInput {
    fn validate_and_normalize(&mut self, _: NaiveDate) -> Result<(), ValidationError> {
        Ok(())
    }
}
/// Lead creation needs only a title; business dimensions are chosen on qualification.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateCrmLeadInput {
    /// Title.
    pub title: String,
    #[serde(default)]
    /// Company name.
    pub company_name: String,
    #[serde(default)]
    /// Contact name.
    pub contact_name: String,
    #[serde(default)]
    /// Contact details.
    pub contact_details: String,
    #[serde(default)]
    /// Source.
    pub source: String,
    #[serde(default)]
    /// Summary.
    pub summary: String,
    #[serde(default)]
    /// Next action.
    pub next_action: String,
    pub next_follow_up: Option<String>,
    /// Customer id.
    pub customer_id: Option<Uuid>,
    pub owner_user_id: Option<Uuid>,
}
/// Append screening history with the current lead version.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordCrmLeadFollowupInput {
    /// Lead id.
    pub lead_id: Uuid,
    pub expected_version: i64,
    /// Note.
    pub note: String,
    /// new, contacting, or disqualified. Converted is not a follow-up state.
    pub status: String,
    #[serde(default)]
    /// Next action.
    pub next_action: String,
    pub next_follow_up: Option<String>,
    #[serde(default)]
    /// Disqualification reason.
    pub disqualification_reason: String,
}
/// Explicitly confirmed conversion; never supply inferred dimensions or commercial values.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConvertCrmLeadInput {
    /// Lead id.
    pub lead_id: Uuid,
    pub expected_version: i64,
    /// Set true only after the human explicitly confirms the displayed conversion facts.
    pub confirmed: bool,
    pub opportunity: QualifiedOpportunityInput,
}
/// Confirmed opportunity fields; initial stage is fixed by the server to contacting.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualifiedOpportunityInput {
    /// Legal entity id.
    pub legal_entity_id: Uuid,
    pub business_unit_id: Uuid,
    /// Customer id.
    pub customer_id: Option<Uuid>,
    pub title: String,
    /// Company name.
    pub company_name: String,
    #[serde(default)]
    /// Contact name.
    pub contact_name: String,
    #[serde(default)]
    /// Contact details.
    pub contact_details: String,
    pub expected_amount_minor: Option<i64>,
    /// Currency.
    pub currency: String,
    #[serde(default)]
    /// Next action.
    pub next_action: String,
    pub next_follow_up: Option<String>,
    /// Owner user id.
    pub owner_user_id: Option<Uuid>,
    pub expected_close_date: Option<String>,
}
