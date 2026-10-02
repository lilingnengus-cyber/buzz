use crate::b2::common::DomainError;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Scoped service register filters, evaluated before pagination.
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filters {
    pub query: Option<String>,
    pub status: Option<String>,
    pub expiry: Option<String>,
    pub today: Option<NaiveDate>,
    #[serde(default)]
    pub offset: i64,
}
/// Create or edit a service project without changing financial documents.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectInput {
    pub title: String,
    pub legal_entity_id: Uuid,
    pub business_unit_id: Uuid,
    pub customer_id: Uuid,
    pub owner_user_id: Uuid,
    #[serde(default)]
    pub contact_name: String,
    pub service_kind: String,
    pub sales_order_line_id: Option<Uuid>,
    pub renewal_of_project_id: Option<Uuid>,
    pub starts_on: Option<NaiveDate>,
    pub ends_on: Option<NaiveDate>,
    pub status: String,
    #[serde(default)]
    pub description: String,
    pub expected_version: Option<i64>,
}
/// Delivery item maintained under its parent project's authorization.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliverableInput {
    pub title: String,
    pub owner_user_id: Uuid,
    pub due_on: Option<NaiveDate>,
    pub status: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub evidence_url: String,
    pub expected_version: Option<i64>,
}
/// Immutable customer acceptance; passing acceptance completes delivery only.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptanceInput {
    pub accepted_on: NaiveDate,
    pub customer_reviewer: String,
    pub result: String,
    pub note: String,
    pub evidence_url: String,
    pub expected_version: i64,
}
pub(super) fn text(value: &str, max: usize, required: bool) -> Result<(), DomainError> {
    if value.chars().count() > max || (required && value.trim().is_empty()) {
        return Err(DomainError::Invalid("字段为空或超过长度限制".into()));
    }
    Ok(())
}
pub(super) fn evidence(value: &str) -> Result<(), DomainError> {
    text(value, 2000, false)?;
    if !value.is_empty() {
        let url =
            url::Url::parse(value).map_err(|_| DomainError::Invalid("无效凭据链接".into()))?;
        if !["http", "https"].contains(&url.scheme())
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(DomainError::Invalid("凭据链接仅支持 HTTP 或 HTTPS".into()));
        }
    }
    Ok(())
}
impl ProjectInput {
    pub(super) fn validate(&self) -> Result<(), DomainError> {
        text(&self.title, 200, true)?;
        text(&self.description, 4000, false)?;
        text(&self.contact_name, 200, false)?;
        if !["technical_service", "software_service"].contains(&self.service_kind.as_str())
            || !["pending", "active", "acceptance", "paused", "cancelled"]
                .contains(&self.status.as_str())
            || self.starts_on.zip(self.ends_on).is_some_and(|(a, b)| b < a)
        {
            return Err(DomainError::Invalid(
                "无效服务类型、状态或起止日期；完成须通过验收".into(),
            ));
        }
        Ok(())
    }
}
