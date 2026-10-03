use crate::b2::common::DomainError;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Editable presales fields. Legal entity is fixed; operating-unit changes recheck access.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveOpportunity {
    pub legal_entity_id: Uuid,
    pub business_unit_id: Uuid,
    pub customer_id: Option<Uuid>,
    pub account_id: Option<Uuid>,
    pub contact_id: Option<Uuid>,
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
    /// Omitted ownership preserves the existing owner; creates default to the caller.
    pub owner_user_id: Option<Uuid>,
    /// Missing preserves the date; explicit null clears it for older-client compatibility.
    #[serde(
        default,
        deserialize_with = "optional_update",
        skip_serializing_if = "Option::is_none"
    )]
    pub expected_close_date: Option<Option<NaiveDate>>,
    pub loss_reason: Option<String>,
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
    pub loss_reason: Option<String>,
}
/// Persisted opportunity returned only within the actor's current scope.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Opportunity {
    pub id: Uuid,
    pub legal_entity_id: Uuid,
    pub business_unit_id: Uuid,
    pub customer_id: Option<Uuid>,
    pub account_id: Option<Uuid>,
    pub contact_id: Option<Uuid>,
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
    pub owner_name: String,
    pub expected_close_date: Option<NaiveDate>,
    pub loss_reason: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}
/// Bounded list filters; due date is explicit in the user's local calendar.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filters {
    pub query: Option<String>,
    pub account_id: Option<Uuid>,
    pub stage: Option<String>,
    pub due_by: Option<NaiveDate>,
    pub followup: Option<String>,
    pub today: Option<NaiveDate>,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub mine: bool,
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

fn optional_update<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(deserializer).map(Some)
}
pub(super) fn loss_reason(
    stage: &str,
    supplied: Option<&str>,
    previous: Option<&str>,
) -> Result<String, DomainError> {
    let reason = supplied.or(previous).unwrap_or("").trim();
    text(reason, 1000, false)?;
    if stage != "lost" {
        return Ok(String::new());
    }
    if reason.is_empty() {
        return Err(DomainError::Invalid("转为已流失时请填写流失原因".into()));
    }
    Ok(reason.to_string())
}
