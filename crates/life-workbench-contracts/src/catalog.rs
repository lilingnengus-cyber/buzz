//! Versioned, immutable tool-to-authority contracts.

use serde::{Deserialize, Serialize};

/// Catalog version shared by the Gateway, MCP, and LifeOS adapter.
pub const CATALOG_VERSION: u16 = 1;

/// Minimum risk assigned by the trusted catalog.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Risk {
    /// Read-only access with no domain mutation.
    Read,
    /// A bounded, non-sensitive append or draft operation.
    Low,
    /// A normal resource create or single-resource mutation.
    Medium,
    /// A destructive, externally visible, sensitive, or policy-changing operation.
    High,
}

/// One immutable fixed-tool contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolContract {
    /// Exact MCP tool name.
    pub name: &'static str,
    /// Exact Life IAM capability required by the operation.
    pub capability: &'static str,
    /// Minimum server-assigned risk.
    pub risk: Risk,
    /// Whether the call must be bound to an optimistic resource version.
    pub requires_expected_version: bool,
    /// Maximum number of resources accepted by one call.
    pub max_batch: u32,
    /// Immutable catalog version.
    pub catalog_version: u16,
}

macro_rules! tool {
    ($name:literal, $capability:literal, $risk:ident, $version:expr, $batch:expr) => {
        ToolContract {
            name: $name,
            capability: $capability,
            risk: Risk::$risk,
            requires_expected_version: $version,
            max_batch: $batch,
            catalog_version: CATALOG_VERSION,
        }
    };
}

static TOOLS: &[ToolContract] = &[
    tool!("get_today_context", "focus:read", Read, false, 100),
    tool!("get_system_overview", "workspace:read", Read, false, 100),
    tool!("list_projects", "project:read", Read, false, 100),
    tool!("get_project_context", "project:read", Read, false, 100),
    tool!("list_actions", "action:read", Read, false, 100),
    tool!("get_action_detail", "action:read", Read, false, 100),
    tool!("search_journal", "journal:read", Read, false, 100),
    tool!("get_review_context", "review:read", Read, false, 100),
    tool!("get_weekly_review_context", "review:read", Read, false, 100),
    tool!("search_knowledge", "knowledge:read", Read, false, 100),
    tool!("get_knowledge_item", "knowledge:read", Read, false, 100),
    tool!(
        "get_ai_execution_context",
        "ai_execution:read",
        Read,
        false,
        100
    ),
    tool!("create_goal", "goal:create", Medium, false, 25),
    tool!("create_project", "project:create", Medium, false, 25),
    tool!("create_action", "action:create", Medium, false, 25),
    tool!("update_action", "action:update", Medium, true, 25),
    tool!(
        "update_action_status",
        "action:status_update",
        Medium,
        true,
        25
    ),
    tool!(
        "reorder_action_children",
        "action:reorder",
        Medium,
        true,
        25
    ),
    tool!("set_today_focus", "focus:replace", Medium, true, 25),
    tool!("create_journal_entry", "journal:create", Medium, false, 25),
    tool!("create_daily_review", "review:create", Medium, false, 25),
    tool!("create_project_review", "review:create", Medium, false, 25),
    tool!("apply_weekly_review", "review:update", Medium, true, 25),
    tool!(
        "create_knowledge_item",
        "knowledge:create",
        Medium,
        false,
        25
    ),
    tool!(
        "start_ai_execution",
        "ai_execution:start",
        Medium,
        false,
        25
    ),
    tool!(
        "append_ai_execution_output",
        "ai_execution:append_output",
        Medium,
        true,
        25
    ),
    tool!(
        "finish_ai_execution",
        "ai_execution:finish",
        Medium,
        true,
        25
    ),
    tool!(
        "preview_life_write",
        "write_command:preview",
        Medium,
        true,
        1
    ),
    // This is an execution envelope, not generic write authority. The
    // delegation remains bound to the confirmed command and original grant.
    tool!(
        "execute_confirmed_life_write",
        "write_command:execute",
        High,
        true,
        1
    ),
];

/// Returns every tool in stable catalog order.
pub fn tools() -> &'static [ToolContract] {
    TOOLS
}

/// Resolves an exact tool name, returning `None` for every unknown tool.
pub fn tool(name: &str) -> Option<&'static ToolContract> {
    TOOLS.iter().find(|entry| entry.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn fixed_catalog_maps_tool_authority_and_rejects_unknown_tools() {
        let update = tool("update_action_status").expect("known tool");
        assert_eq!(update.capability, "action:status_update");
        assert_eq!(update.risk, Risk::Medium);
        assert!(update.requires_expected_version);

        let confirmed = tool("execute_confirmed_life_write").expect("confirmed tool");
        assert_eq!(confirmed.risk, Risk::High);
        assert_eq!(confirmed.max_batch, 1);
        assert!(tool("run_sql").is_none());
        assert!(tool("fetch_url").is_none());
    }

    #[test]
    fn catalog_is_complete_unique_and_versioned() {
        let expected_names = BTreeSet::from([
            "get_today_context",
            "get_system_overview",
            "list_projects",
            "get_project_context",
            "list_actions",
            "get_action_detail",
            "search_journal",
            "get_review_context",
            "get_weekly_review_context",
            "search_knowledge",
            "get_knowledge_item",
            "get_ai_execution_context",
            "create_goal",
            "create_project",
            "create_action",
            "update_action",
            "update_action_status",
            "reorder_action_children",
            "set_today_focus",
            "create_journal_entry",
            "create_daily_review",
            "create_project_review",
            "apply_weekly_review",
            "create_knowledge_item",
            "start_ai_execution",
            "append_ai_execution_output",
            "finish_ai_execution",
            "preview_life_write",
            "execute_confirmed_life_write",
        ]);
        let names = tools()
            .iter()
            .map(|entry| entry.name)
            .collect::<BTreeSet<_>>();
        assert_eq!(names, expected_names);
        assert_eq!(names.len(), tools().len());
        assert!(tools().iter().all(|entry| {
            entry.catalog_version == CATALOG_VERSION
                && entry.max_batch > 0
                && entry
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
                && entry.capability.contains(':')
        }));
        assert!(tools()
            .iter()
            .filter(|entry| entry.risk == Risk::Read)
            .all(|entry| !entry.requires_expected_version));
    }
}
