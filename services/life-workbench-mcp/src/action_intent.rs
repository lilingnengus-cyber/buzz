//! Compile the proxy's structured extraction into one bounded action write.
//! Natural-language interpretation belongs to the agent; this boundary never
//! guesses identifiers, relative dates, or additional operations.

use crate::tools::{optional_due_date, safe_id, Invocation, ToolInputError};
use chrono::NaiveDate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateActionInput {
    workspace_id: String,
    project_id: String,
    parent_id: Option<String>,
    title: String,
    /// Titles of up to 20 direct children, created atomically with this action.
    child_titles: Option<Vec<String>>,
    note: Option<String>,
    priority: Option<String>,
    /// YYYY-MM-DD or an RFC3339 instant with explicit timezone, e.g. 2026-09-06T10:00:00+08:00.
    due_date: Option<String>,
    focus_date: Option<String>,
    estimate_min: Option<i32>,
    /// Optional user-provided UUID, forwarded outside the business input.
    idempotency_key: Option<String>,
}

impl CreateActionInput {
    pub(crate) fn compile(self) -> Result<Invocation, ToolInputError> {
        safe_id(&self.project_id)?;
        if let Some(parent) = &self.parent_id {
            safe_id(parent)?;
        }
        if !valid_text(&self.title, 200)
            || self
                .note
                .as_deref()
                .is_some_and(|note| !valid_text(note, 10_000))
        {
            return Err(ToolInputError);
        }
        if self.child_titles.as_ref().is_some_and(|titles| {
            titles.len() > 20 || titles.iter().any(|title| !valid_text(title, 200))
        }) {
            return Err(ToolInputError);
        }
        optional_due_date(self.due_date.as_deref())?;
        for value in self.focus_date.iter() {
            let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| ToolInputError)?;
            if date.format("%Y-%m-%d").to_string() != *value {
                return Err(ToolInputError);
            }
        }
        let priority = self.priority.as_deref().unwrap_or("MEDIUM");
        if !matches!(priority, "LOW" | "MEDIUM" | "HIGH")
            || self
                .estimate_min
                .is_some_and(|value| !(1..=1440).contains(&value))
        {
            return Err(ToolInputError);
        }
        let idempotency_key = self
            .idempotency_key
            .as_deref()
            .map(|value| Uuid::parse_str(value).map_err(|_| ToolInputError))
            .transpose()?;
        let mut value = json!({
            "projectId":self.project_id,"parentId":self.parent_id,
            "title":self.title,"note":self.note,"priority":priority,
            "dueDate":self.due_date,"focusDate":self.focus_date,
            "estimateMin":self.estimate_min,"childTitles":self.child_titles
        });
        if let Some(fields) = value.as_object_mut() {
            fields.retain(|_, value| !value.is_null());
        }
        let mut invocation = Invocation::write(
            "create_action",
            "/api/workbench/actions/write",
            "workspace",
            self.workspace_id,
            None,
            json!({"operation":"create","value":value}),
        )?;
        invocation.idempotency_key = idempotency_key;
        Ok(invocation)
    }
}

fn valid_text(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.encode_utf16().count() <= max
        && !value.chars().any(|ch| ch <= '\u{1f}' || ch == '\u{7f}')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_or_unbounded_children_before_writing() {
        for titles in [
            json!([""]),
            json!([" bad"]),
            json!(["bad\nname"]),
            json!(vec!["child"; 21]),
        ] {
            let input: CreateActionInput = serde_json::from_value(json!({"workspaceId":"workspace-1","projectId":"project-1","title":"parent","childTitles":titles})).unwrap();
            assert!(input.compile().is_err());
        }
    }
}
