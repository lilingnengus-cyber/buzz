use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
/// Lead fields before qualification; commercial scope is chosen on conversion.
#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveLead {
    pub title: String,
    #[serde(default)]
    pub company_name: String,
    #[serde(default)]
    pub contact_name: String,
    #[serde(default)]
    pub contact_details: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub next_action: String,
    pub next_follow_up: Option<NaiveDate>,
    pub customer_id: Option<Uuid>,
    pub owner_user_id: Option<Uuid>,
    pub expected_version: Option<i64>,
}
/// Immutable follow-up with a screening decision.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeadFollowup {
    pub note: String,
    pub status: String,
    pub next_action: String,
    pub next_follow_up: Option<NaiveDate>,
    #[serde(default)]
    pub disqualification_reason: String,
    pub expected_version: i64,
}
/// Versioned lead to opportunity conversion.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConvertLead {
    pub expected_version: i64,
    pub opportunity: super::super::SaveOpportunity,
}
/// Persisted lead, exposed only to its owner.
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Lead {
    pub id: Uuid,
    pub title: String,
    pub company_name: String,
    pub contact_name: String,
    pub contact_details: String,
    pub source: String,
    pub summary: String,
    pub next_action: String,
    pub next_follow_up: Option<NaiveDate>,
    pub status: String,
    pub disqualification_reason: String,
    pub customer_id: Option<Uuid>,
    pub owner_user_id: Uuid,
    pub owner_name: String,
    pub converted_opportunity_id: Option<Uuid>,
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
/// Search and screening filters.
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeadFilters {
    pub query: Option<String>,
    pub status: Option<String>,
    pub owner_user_id: Option<Uuid>,
    pub due_by: Option<NaiveDate>,
    #[serde(default)]
    pub offset: i64,
}
use super::super::model::text;
use crate::b2::common::DomainError;
impl SaveLead {
    pub(super) fn validate(&self) -> Result<(), DomainError> {
        text(&self.title, 160, true)?;
        text(&self.company_name, 160, false)?;
        text(&self.contact_name, 100, false)?;
        text(&self.contact_details, 200, false)?;
        text(&self.source, 100, false)?;
        text(&self.summary, 4000, false)?;
        text(&self.next_action, 500, false)
    }
}
impl LeadFollowup {
    pub(super) fn validate(&self) -> Result<(), DomainError> {
        text(&self.note, 4000, true)?;
        text(&self.next_action, 500, false)?;
        if !["new", "contacting", "disqualified"].contains(&self.status.as_str()) {
            return Err(DomainError::Invalid("无效线索状态".into()));
        }
        text(
            &self.disqualification_reason,
            1000,
            self.status == "disqualified",
        )
    }
}
pub(super) fn validate_filters(f: &LeadFilters) -> Result<(), DomainError> {
    text(f.query.as_deref().unwrap_or(""), 160, false)?;
    if !(0..=100000).contains(&f.offset)
        || f.status
            .as_deref()
            .is_some_and(|s| !["new", "contacting", "converted", "disqualified"].contains(&s))
    {
        return Err(DomainError::Invalid("无效筛选".into()));
    }
    Ok(())
}
