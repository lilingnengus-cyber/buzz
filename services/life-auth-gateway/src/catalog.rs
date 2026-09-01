//! Versioned, fail-closed LifeOS capability and tool catalog.

use serde_json::Value;
use sqlx::{PgPool, Row};
use std::collections::{BTreeMap, BTreeSet};

/// Catalog version understood by this Gateway build.
pub const CATALOG_VERSION: i32 = 1;

/// Stable risk classification whose ordering may only increase across versions.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RiskClass {
    /// Read-only or acknowledgement-level impact.
    Low,
    /// Reversible creation or ordinary mutation.
    Medium,
    /// Destructive, externally visible, export, or policy mutation.
    High,
}

impl RiskClass {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }
}

/// One immutable entry in the compiled Life capability catalog.
#[derive(Clone, Copy, Debug)]
pub struct CatalogEntry {
    /// Stable capability identifier.
    pub capability: &'static str,
    /// Fixed MCP tool names that require this capability.
    pub allowed_tools: &'static [&'static str],
    /// Minimum operational risk.
    pub risk_class: RiskClass,
    /// Whether mutation calls must carry an optimistic version.
    pub requires_expected_version: bool,
    /// Default per-turn call ceiling.
    pub default_max_calls: u32,
    /// Hard per-call batch ceiling.
    pub max_batch_size: u32,
    /// Required policy obligations, encoded with stable snake-case names.
    pub obligations: &'static [&'static str],
    /// Immutable catalog version.
    pub catalog_version: i32,
}

/// Fixed tool-to-capability binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolBinding {
    /// Fixed MCP tool name.
    pub tool: &'static str,
    /// Required capability.
    pub capability: &'static str,
}

/// Catalog validation failure safe for startup reporting.
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    /// The compiled or persisted catalog violates an invariant.
    #[error("Life capability catalog is invalid")]
    Invalid,
    /// PostgreSQL could not be queried.
    #[error("Life capability catalog is unavailable")]
    Database,
}

const NONE: &[&str] = &[];
const CONFIRM_STEP_UP: &[&str] = &["human_confirmation", "step_up_authentication"];

macro_rules! entry {
    ($cap:literal, [$($tool:literal),*], $risk:ident, $version:expr, $calls:expr, $batch:expr, $obligations:expr) => {
        CatalogEntry { capability: $cap, allowed_tools: &[$($tool),*], risk_class: RiskClass::$risk,
            requires_expected_version: $version, default_max_calls: $calls,
            max_batch_size: $batch, obligations: $obligations, catalog_version: CATALOG_VERSION }
    };
}

static ENTRIES: &[CatalogEntry] = &[
    entry!(
        "workspace:read",
        ["get_system_overview"],
        Low,
        false,
        100,
        100,
        NONE
    ),
    entry!("domain:read", [], Low, false, 100, 100, NONE),
    entry!("domain:create", [], Medium, false, 25, 25, NONE),
    entry!("domain:update", [], Medium, true, 25, 25, NONE),
    entry!("goal:read", [], Low, false, 100, 100, NONE),
    entry!("goal:create", ["create_goal"], Medium, false, 25, 25, NONE),
    entry!("goal:update", [], Medium, true, 25, 25, NONE),
    entry!("goal:archive", [], High, true, 5, 10, CONFIRM_STEP_UP),
    entry!(
        "project:read",
        ["list_projects", "get_project_context"],
        Low,
        false,
        100,
        100,
        NONE
    ),
    entry!(
        "project:create",
        ["create_project"],
        Medium,
        false,
        25,
        25,
        NONE
    ),
    entry!("project:update", [], Medium, true, 25, 25, NONE),
    entry!("project:archive", [], High, true, 5, 10, CONFIRM_STEP_UP),
    entry!(
        "action:read",
        ["list_actions", "get_action_detail"],
        Low,
        false,
        100,
        100,
        NONE
    ),
    entry!(
        "action:create",
        ["create_action"],
        Medium,
        false,
        25,
        25,
        NONE
    ),
    entry!(
        "action:update",
        ["update_action"],
        Medium,
        true,
        25,
        25,
        NONE
    ),
    entry!(
        "action:status_update",
        ["update_action_status"],
        Medium,
        true,
        25,
        25,
        NONE
    ),
    entry!(
        "action:reorder",
        ["reorder_action_children"],
        Medium,
        true,
        25,
        25,
        NONE
    ),
    entry!("action:delete", [], High, true, 5, 10, CONFIRM_STEP_UP),
    entry!(
        "focus:read",
        ["get_today_context"],
        Low,
        false,
        100,
        100,
        NONE
    ),
    entry!("focus:update", [], Medium, true, 25, 25, NONE),
    entry!(
        "focus:replace",
        ["set_today_focus"],
        Medium,
        true,
        25,
        25,
        NONE
    ),
    entry!("calendar:read", [], Low, false, 100, 100, NONE),
    entry!("calendar:create", [], Medium, false, 25, 25, NONE),
    entry!("calendar:update", [], Medium, true, 25, 25, NONE),
    entry!("calendar:delete", [], High, true, 5, 10, CONFIRM_STEP_UP),
    entry!("calendar:invite", [], High, true, 5, 10, CONFIRM_STEP_UP),
    entry!(
        "journal:read",
        ["search_journal"],
        Low,
        false,
        100,
        100,
        NONE
    ),
    entry!(
        "journal:create",
        ["create_journal_entry"],
        Medium,
        false,
        25,
        25,
        NONE
    ),
    entry!("journal:update", [], Medium, true, 25, 25, NONE),
    entry!("journal:delete", [], High, true, 5, 10, CONFIRM_STEP_UP),
    entry!(
        "knowledge:read",
        ["search_knowledge", "get_knowledge_item"],
        Low,
        false,
        100,
        100,
        NONE
    ),
    entry!(
        "knowledge:create",
        ["create_knowledge_item"],
        Medium,
        false,
        25,
        25,
        NONE
    ),
    entry!("knowledge:update", [], Medium, true, 25, 25, NONE),
    entry!("knowledge:delete", [], High, true, 5, 10, CONFIRM_STEP_UP),
    entry!("knowledge:export", [], High, false, 5, 10, CONFIRM_STEP_UP),
    entry!(
        "review:read",
        ["get_review_context", "get_weekly_review_context"],
        Low,
        false,
        100,
        100,
        NONE
    ),
    entry!(
        "review:create",
        ["create_daily_review", "create_project_review"],
        Medium,
        false,
        25,
        25,
        NONE
    ),
    entry!(
        "review:update",
        ["apply_weekly_review"],
        Medium,
        true,
        25,
        25,
        NONE
    ),
    entry!(
        "ai_execution:read",
        ["get_ai_execution_context"],
        Low,
        false,
        100,
        100,
        NONE
    ),
    entry!(
        "ai_execution:start",
        ["start_ai_execution"],
        Medium,
        false,
        25,
        25,
        NONE
    ),
    entry!(
        "ai_execution:append_output",
        ["append_ai_execution_output"],
        Medium,
        true,
        25,
        25,
        NONE
    ),
    entry!(
        "ai_execution:finish",
        ["finish_ai_execution"],
        Medium,
        true,
        25,
        25,
        NONE
    ),
    entry!(
        "ai_execution:policy_update",
        [],
        High,
        true,
        5,
        10,
        CONFIRM_STEP_UP
    ),
    entry!(
        "write_command:preview",
        ["preview_life_write"],
        Medium,
        true,
        5,
        1,
        NONE
    ),
    entry!(
        "write_command:execute",
        ["execute_confirmed_life_write"],
        High,
        true,
        1,
        1,
        CONFIRM_STEP_UP
    ),
    entry!("notification:read", [], Low, false, 100, 100, NONE),
    entry!("notification:acknowledge", [], Medium, true, 25, 25, NONE),
];

/// Returns the complete catalog compiled into this Gateway build.
pub fn entries() -> &'static [CatalogEntry] {
    ENTRIES
}

/// Resolves a known capability or fails closed with `None`.
pub fn capability(name: &str) -> Option<&'static CatalogEntry> {
    ENTRIES.iter().find(|entry| entry.capability == name)
}

/// Resolves a fixed MCP tool or fails closed with `None`.
pub fn tool(name: &str) -> Option<ToolBinding> {
    ENTRIES.iter().find_map(|entry| {
        entry.allowed_tools.contains(&name).then_some(ToolBinding {
            tool: name_static(entry, name),
            capability: entry.capability,
        })
    })
}

fn name_static(entry: &'static CatalogEntry, name: &str) -> &'static str {
    entry
        .allowed_tools
        .iter()
        .copied()
        .find(|tool| *tool == name)
        .unwrap_or("")
}

/// Validates static uniqueness and the exact active persisted catalog at startup.
pub async fn validate_persisted(pool: &PgPool) -> Result<(), CatalogError> {
    validate_compiled()?;
    let rows = sqlx::query(
        "SELECT capability,allowed_tools,risk_class,requires_expected_version,
                default_max_calls,max_batch_size,obligations,catalog_version,status
         FROM life_capability_catalog ORDER BY capability,catalog_version",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| CatalogError::Database)?;
    let mut previous = BTreeMap::<String, RiskClass>::new();
    let mut active = BTreeMap::<String, usize>::new();
    for row in &rows {
        let name: String = row.get("capability");
        let risk_text: String = row.get("risk_class");
        let risk = RiskClass::parse(&risk_text).ok_or(CatalogError::Invalid)?;
        if previous.get(&name).is_some_and(|old| risk < *old) {
            return Err(CatalogError::Invalid);
        }
        previous.insert(name.clone(), risk);
        if row.get::<String, _>("status") == "active" {
            *active.entry(name).or_default() += 1;
        }
    }
    if active.len() != ENTRIES.len() || active.values().any(|count| *count != 1) {
        return Err(CatalogError::Invalid);
    }
    for entry in ENTRIES {
        let row = rows
            .iter()
            .find(|row| {
                row.get::<String, _>("capability") == entry.capability
                    && row.get::<i32, _>("catalog_version") == entry.catalog_version
                    && row.get::<String, _>("status") == "active"
            })
            .ok_or(CatalogError::Invalid)?;
        let tools = strings(row.get("allowed_tools"))?;
        let obligations = strings(row.get("obligations"))?;
        if tools
            != entry
                .allowed_tools
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>()
            || obligations
                != entry
                    .obligations
                    .iter()
                    .map(|value| (*value).to_owned())
                    .collect::<Vec<_>>()
            || row.get::<String, _>("risk_class") != entry.risk_class.as_str()
            || row.get::<bool, _>("requires_expected_version") != entry.requires_expected_version
            || row.get::<i32, _>("default_max_calls") != entry.default_max_calls as i32
            || row.get::<i32, _>("max_batch_size") != entry.max_batch_size as i32
        {
            return Err(CatalogError::Invalid);
        }
    }
    Ok(())
}

fn validate_compiled() -> Result<(), CatalogError> {
    let mut capabilities = BTreeSet::new();
    let mut tools = BTreeSet::new();
    for entry in ENTRIES {
        if !capabilities.insert(entry.capability)
            || entry.default_max_calls == 0
            || entry.default_max_calls > 1_000
            || entry.max_batch_size == 0
            || entry.max_batch_size > 10_000
            || entry.allowed_tools.iter().any(|tool| !tools.insert(*tool))
            || (is_mutation(entry.capability) && !entry.requires_expected_version)
        {
            return Err(CatalogError::Invalid);
        }
    }
    Ok(())
}

fn is_mutation(capability: &str) -> bool {
    !capability.ends_with(":read")
        && !capability.ends_with(":create")
        && !capability.ends_with(":start")
        && capability != "knowledge:export"
}

fn strings(value: Value) -> Result<Vec<String>, CatalogError> {
    serde_json::from_value(value).map_err(|_| CatalogError::Invalid)
}
