//! Shared filters and metadata for permission-scoped master registers.
use crate::b2::DomainError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Optional filters applied before pagination; old callers retain their defaults.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterPageFilter {
    /// Literal, case-insensitive search across supported business fields.
    #[serde(default)]
    pub query: String,
    /// Active or disabled records; omission returns both.
    pub status: Option<String>,
    /// Number of matching records to skip.
    #[serde(default)]
    pub offset: i64,
    /// Exact record lookup within the caller's authorized scope.
    pub id: Option<Uuid>,
}
impl MasterPageFilter {
    /// Reject invalid filters instead of silently changing their meaning.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.offset < 0 || self.query.chars().count() > 200 {
            return Err(DomainError::Invalid(
                "invalid master-data pagination or query".into(),
            ));
        }
        if self
            .status
            .as_deref()
            .is_some_and(|s| !matches!(s, "active" | "disabled"))
        {
            return Err(DomainError::Invalid("invalid master-data status".into()));
        }
        Ok(())
    }
}

/// Counts contain only records visible to the current actor.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterPageMetadata {
    /// Matching records before limit and offset.
    pub total: i64,
    /// Whether another page is available.
    pub has_more: bool,
    /// Unfiltered authorized record count for each category.
    pub counts: BTreeMap<String, i64>,
}
