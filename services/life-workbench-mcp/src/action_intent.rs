//! Compile the proxy's structured extraction into one bounded action write.
//! Natural-language interpretation belongs to the agent; this boundary never
//! guesses identifiers, relative dates, or additional operations.

use crate::tools::{safe_id, Invocation, ToolInputError};
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
    note: Option<String>,
    priority: Option<String>,
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
        for value in [&self.due_date, &self.focus_date].into_iter().flatten() {
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
            "estimateMin":self.estimate_min
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
