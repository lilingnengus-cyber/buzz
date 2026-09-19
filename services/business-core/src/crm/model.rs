use crate::b2::common::DomainError;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Editable presales fields. Scope is fixed after creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveOpportunity {
    pub legal_entity_id: Uuid,
    pub business_unit_id: Uuid,
    pub customer_id: Option<Uuid>,
    pub title: String,
    pub company_name: String,
    #[serde(default)]
    pub contact_name: String,
    #[serde(default)]
    pub contact_details: String,
    pub stage: String,
    pub expected_amount_minor: Option<i64>,
    pub currency: String,
    #[serde(default)]
    pub next_action: String,
    pub next_follow_up: Option<NaiveDate>,
    pub expected_version: Option<i64>,
}
/// A follow-up and the resulting next step, committed atomically.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddFollowup {
    pub note: String,
    pub stage: String,
    pub next_action: String,
    pub next_follow_up: Option<NaiveDate>,
    pub expected_version: i64,
}
/// Persisted opportunity returned only within the actor's current scope.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Opportunity {
    pub id: Uuid,
    pub legal_entity_id: Uuid,
    pub business_unit_id: Uuid,
    pub customer_id: Option<Uuid>,
    pub title: String,
    pub company_name: String,
    pub contact_name: String,
    pub contact_details: String,
    pub stage: String,
    pub expected_amount_minor: Option<i64>,
    pub currency: String,
    pub next_action: String,
    pub next_follow_up: Option<NaiveDate>,
    pub owner_user_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}
/// Bounded list filters; due date is explicit in the user's local calendar.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filters {
    pub query: Option<String>,
    pub stage: Option<String>,
    pub due_by: Option<NaiveDate>,
    #[serde(default)]
    pub offset: i64,
}
pub(super) fn text(value: &str, max: usize, required: bool) -> Result<(), DomainError> {
    if value.chars().count() > max || (required && value.trim().is_empty()) {
        return Err(DomainError::Invalid("请检查必填内容和文字长度".into()));
    }
    Ok(())
}
pub(super) fn stage(value: &str) -> Result<(), DomainError> {
    if !["new", "contacting", "quoting", "won", "lost"].contains(&value) {
        return Err(DomainError::Invalid("未知商机阶段".into()));
    }
    Ok(())
}
impl SaveOpportunity {
    pub(super) fn validate(&self) -> Result<(), DomainError> {
        text(&self.title, 160, true)?;
        text(&self.company_name, 160, true)?;
        text(&self.contact_name, 100, false)?;
        text(&self.contact_details, 200, false)?;
        text(&self.next_action, 500, false)?;
        stage(&self.stage)?;
        crate::b2::common::validate_currency(&self.currency)?;
        if self
            .expected_amount_minor
            .is_some_and(|n| !(0..=999999999999).contains(&n))
        {
            return Err(DomainError::Invalid("预计金额超出范围".into()));
        }
        Ok(())
    }
}
