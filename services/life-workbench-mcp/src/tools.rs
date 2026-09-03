use chrono::NaiveDate;
use life_workbench_contracts::{catalog, normalized_input_hash};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const DEFAULT_LIST_LIMIT: u32 = 50;
const DEFAULT_SNIPPET_LENGTH: u32 = 500;
const DEFAULT_GOAL_HORIZON: &str = "QUARTER";
const DEFAULT_PROJECT_COLOR: &str = "#197b70";
const DEFAULT_ACTION_PRIORITY: &str = "MEDIUM";
const DEFAULT_FOCUS_MODE: &str = "append";
const DEFAULT_KNOWLEDGE_TYPE: &str = "NOTE";
const DEFAULT_KNOWLEDGE_STATUS: &str = "APPROVED";
const DEFAULT_AI_RISK_LEVEL: &str = "LOW";

pub(crate) const READ_TOOL_NAMES: [&str; 12] = [
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
];

pub(crate) const WRITE_TOOL_NAMES: [&str; 17] = [
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
];

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceContext {
    #[serde(rename = "type")]
    pub(crate) resource_type: String,
    pub(crate) id: String,
    pub(crate) expected_version: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) preview_hash: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Invocation {
    pub(crate) tool: &'static str,
    pub(crate) capability: &'static str,
    pub(crate) route: String,
    pub(crate) resource: Option<ResourceContext>,
    pub(crate) api_input: Value,
    pub(crate) normalized_input_hash: String,
    pub(crate) is_write: bool,
}

impl Invocation {
    fn read(
        tool: &'static str,
        route: String,
        resource_type: &str,
        resource_id: String,
        api_input: Value,
    ) -> Result<Self, ToolInputError> {
        safe_id(&resource_id)?;
        let contract = catalog::tool(tool).ok_or(ToolInputError)?;
        if !READ_TOOL_NAMES.contains(&tool) {
            return Err(ToolInputError);
        }
        Ok(Self {
            tool,
            capability: contract.capability,
            route,
            resource: Some(ResourceContext {
                resource_type: resource_type.to_owned(),
                id: resource_id,
                expected_version: None,
                preview_hash: None,
            }),
            normalized_input_hash: normalized_input_hash(&api_input).map_err(|_| ToolInputError)?,
            api_input,
            is_write: false,
        })
    }

    fn write(
        tool: &'static str,
        route: &str,
        resource_type: &str,
        resource_id: String,
        expected_version: Option<i64>,
        api_input: Value,
    ) -> Result<Self, ToolInputError> {
        safe_id(&resource_id)?;
        let contract = catalog::tool(tool).ok_or(ToolInputError)?;
        if !WRITE_TOOL_NAMES.contains(&tool)
            || tool == "execute_confirmed_life_write"
            || contract.risk == catalog::Risk::Read
            || contract.requires_expected_version != expected_version.is_some()
            || expected_version.is_some_and(|version| version < 1)
        {
            return Err(ToolInputError);
        }
        validate_bounded_value(&api_input, 0)?;
        Ok(Self {
            tool,
            capability: contract.capability,
            route: route.into(),
            resource: Some(ResourceContext {
                resource_type: resource_type.into(),
                id: resource_id,
                expected_version,
                preview_hash: None,
            }),
            normalized_input_hash: normalized_input_hash(&api_input).map_err(|_| ToolInputError)?,
            api_input,
            is_write: true,
        })
    }

    fn confirmed() -> Result<Self, ToolInputError> {
        let tool = "execute_confirmed_life_write";
        let contract = catalog::tool(tool).ok_or(ToolInputError)?;
        if contract.capability != "write_command:execute" {
            return Err(ToolInputError);
        }
        let api_input = json!({});
        Ok(Self {
            tool,
            capability: contract.capability,
            route: "/api/workbench/write-commands/execute".into(),
            resource: None,
            normalized_input_hash: normalized_input_hash(&api_input).map_err(|_| ToolInputError)?,
            api_input,
            is_write: true,
        })
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TodayInput {
    pub(crate) workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) date: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkspaceInput {
    pub(crate) workspace_id: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProjectListInput {
    pub(crate) workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) archived: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) limit: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProjectInput {
    pub(crate) project_id: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ActionListInput {
    pub(crate) workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) statuses: Option<Vec<ActionStatus>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) limit: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum ActionStatus {
    Pending,
    Doing,
    Blocked,
    Done,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ActionInput {
    pub(crate) action_id: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct JournalSearchInput {
    pub(crate) workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) snippet_length: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReviewContextInput {
    pub(crate) workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) domain_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) snippet_length: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WeeklyReviewInput {
    pub(crate) workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) week_start: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct KnowledgeSearchInput {
    pub(crate) workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<KnowledgeStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) snippet_length: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum KnowledgeStatus {
    Pending,
    Approved,
    Archived,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct KnowledgeInput {
    pub(crate) knowledge_id: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AiExecutionInput {
    pub(crate) ai_execution_id: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateGoalInput {
    pub(crate) workspace_id: String,
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    pub(crate) domain_id: Option<String>,
    pub(crate) parent_id: Option<String>,
    pub(crate) horizon: Option<String>,
    pub(crate) starts_at: Option<String>,
    pub(crate) ends_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateProjectInput {
    pub(crate) workspace_id: String,
    pub(crate) name: String,
    pub(crate) purpose: Option<String>,
    pub(crate) domain_id: Option<String>,
    pub(crate) goal_id: Option<String>,
    pub(crate) color: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateActionInput {
    pub(crate) workspace_id: String,
    pub(crate) project_id: String,
    pub(crate) parent_id: Option<String>,
    pub(crate) title: String,
    pub(crate) note: Option<String>,
    pub(crate) priority: Option<String>,
    pub(crate) due_date: Option<String>,
    pub(crate) focus_date: Option<String>,
    pub(crate) estimate_min: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UpdateActionInput {
    pub(crate) action_id: String,
    pub(crate) expected_version: i64,
    pub(crate) title: Option<String>,
    pub(crate) note: Option<String>,
    pub(crate) priority: Option<String>,
    pub(crate) due_date: Option<String>,
    pub(crate) focus_date: Option<String>,
    pub(crate) estimate_min: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ActionStatusWriteInput {
    pub(crate) action_id: String,
    pub(crate) expected_version: i64,
    pub(crate) status: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct VersionedId {
    pub(crate) id: String,
    pub(crate) expected_version: i64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReorderActionsInput {
    pub(crate) parent_action_id: String,
    pub(crate) expected_version: i64,
    pub(crate) children: Vec<VersionedId>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FocusWriteInput {
    pub(crate) workspace_id: String,
    pub(crate) membership_version: i64,
    pub(crate) date: String,
    pub(crate) mode: Option<String>,
    pub(crate) actions: Vec<VersionedId>,
    pub(crate) current: Option<Vec<VersionedId>>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct JournalWriteInput {
    pub(crate) workspace_id: String,
    pub(crate) title: String,
    pub(crate) content: String,
    pub(crate) mood: Option<String>,
    pub(crate) energy: Option<i32>,
    pub(crate) entry_date: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DailyReviewWriteInput {
    pub(crate) workspace_id: String,
    pub(crate) title: String,
    pub(crate) content: String,
    pub(crate) happened_on: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProjectReviewWriteInput {
    pub(crate) project_id: String,
    pub(crate) title: String,
    pub(crate) content: String,
    pub(crate) happened_on: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WeeklyReviewWriteInput {
    pub(crate) review_id: String,
    pub(crate) expected_version: i64,
    pub(crate) title: Option<String>,
    pub(crate) content: String,
    pub(crate) happened_on: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct KnowledgeWriteInput {
    pub(crate) workspace_id: String,
    pub(crate) project_id: Option<String>,
    pub(crate) title: String,
    pub(crate) r#type: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) summary: Option<String>,
    pub(crate) content: String,
    pub(crate) tags: Option<Vec<String>>,
    pub(crate) source: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StartAiExecutionInput {
    pub(crate) action_id: String,
    pub(crate) risk_level: Option<String>,
    pub(crate) action_type: String,
    pub(crate) reason: Option<String>,
    pub(crate) plan: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AppendAiOutputInput {
    pub(crate) ai_execution_id: String,
    pub(crate) expected_version: i64,
    pub(crate) r#type: String,
    pub(crate) title: String,
    pub(crate) content: Option<String>,
    pub(crate) data: Option<Value>,
    pub(crate) source_urls: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FinishAiExecutionInput {
    pub(crate) ai_execution_id: String,
    pub(crate) expected_version: i64,
    pub(crate) status: String,
    pub(crate) error: Option<String>,
    pub(crate) block_reason: Option<String>,
    pub(crate) notification_summary: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HighRiskOperation {
    DeleteAction,
    ArchiveProject,
    DeleteJournal,
    DeleteKnowledge,
    ExportKnowledge,
}

impl HighRiskOperation {
    fn resource_type(self) -> &'static str {
        match self {
            Self::DeleteAction => "action",
            Self::ArchiveProject => "project",
            Self::DeleteJournal => "journal",
            Self::DeleteKnowledge | Self::ExportKnowledge => "knowledge",
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PreviewLifeWriteInput {
    pub(crate) operation: HighRiskOperation,
    pub(crate) resource_id: String,
    pub(crate) expected_version: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) include_history: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EmptyInput {}

pub(crate) fn parse_invocation(tool: &str, arguments: Value) -> Result<Invocation, ToolInputError> {
    match tool {
        "get_today_context" => {
            let input: TodayInput = strict(arguments)?;
            optional_date(input.date.as_deref())?;
            Invocation::read(
                "get_today_context",
                "/api/workbench/context/today".into(),
                "workspace",
                input.workspace_id,
                without_nulls(json!({"date": input.date})),
            )
        }
        "get_system_overview" => {
            let input: WorkspaceInput = strict(arguments)?;
            Invocation::read(
                "get_system_overview",
                "/api/workbench/context/system".into(),
                "workspace",
                input.workspace_id,
                json!({}),
            )
        }
        "list_projects" => {
            let input: ProjectListInput = strict(arguments)?;
            bounded_limit(input.limit)?;
            Invocation::read(
                "list_projects",
                "/api/workbench/projects".into(),
                "workspace",
                input.workspace_id,
                json!({
                    "archived": input.archived.unwrap_or(false),
                    "limit": input.limit.unwrap_or(DEFAULT_LIST_LIMIT)
                }),
            )
        }
        "get_project_context" => {
            let input: ProjectInput = strict(arguments)?;
            let id = input.project_id;
            Invocation::read(
                "get_project_context",
                format!("/api/workbench/projects/{id}"),
                "project",
                id,
                json!({}),
            )
        }
        "list_actions" => {
            let input: ActionListInput = strict(arguments)?;
            validate_window(input.from.as_deref(), input.to.as_deref())?;
            bounded_limit(input.limit)?;
            optional_safe_id(input.project_id.as_deref())?;
            Invocation::read(
                "list_actions",
                "/api/workbench/actions".into(),
                "workspace",
                input.workspace_id,
                without_nulls(json!({
                    "projectId": input.project_id,
                    "statuses": input.statuses,
                    "from": input.from,
                    "to": input.to,
                    "limit": input.limit.unwrap_or(DEFAULT_LIST_LIMIT)
                })),
            )
        }
        "get_action_detail" => {
            let input: ActionInput = strict(arguments)?;
            let id = input.action_id;
            Invocation::read(
                "get_action_detail",
                format!("/api/workbench/actions/{id}"),
                "action",
                id,
                json!({}),
            )
        }
        "search_journal" => {
            let input: JournalSearchInput = strict(arguments)?;
            validate_window(input.from.as_deref(), input.to.as_deref())?;
            validate_search(input.query.as_deref())?;
            bounded_limit(input.limit)?;
            bounded_snippet(input.snippet_length)?;
            Invocation::read(
                "search_journal",
                "/api/workbench/journal/search".into(),
                "workspace",
                input.workspace_id,
                without_nulls(json!({
                    "query": input.query,
                    "from": input.from,
                    "to": input.to,
                    "limit": input.limit.unwrap_or(DEFAULT_LIST_LIMIT),
                    "snippetLength": input.snippet_length.unwrap_or(DEFAULT_SNIPPET_LENGTH)
                })),
            )
        }
        "get_review_context" => {
            let input: ReviewContextInput = strict(arguments)?;
            validate_window(input.from.as_deref(), input.to.as_deref())?;
            optional_safe_id(input.project_id.as_deref())?;
            optional_safe_id(input.domain_id.as_deref())?;
            bounded_limit(input.limit)?;
            bounded_snippet(input.snippet_length)?;
            Invocation::read(
                "get_review_context",
                "/api/workbench/reviews/context".into(),
                "workspace",
                input.workspace_id,
                without_nulls(json!({
                    "projectId": input.project_id,
                    "domainId": input.domain_id,
                    "from": input.from,
                    "to": input.to,
                    "limit": input.limit.unwrap_or(DEFAULT_LIST_LIMIT),
                    "snippetLength": input.snippet_length.unwrap_or(DEFAULT_SNIPPET_LENGTH)
                })),
            )
        }
        "get_weekly_review_context" => {
            let input: WeeklyReviewInput = strict(arguments)?;
            optional_date(input.week_start.as_deref())?;
            Invocation::read(
                "get_weekly_review_context",
                "/api/workbench/reviews/weekly".into(),
                "workspace",
                input.workspace_id,
                without_nulls(json!({"weekStart": input.week_start})),
            )
        }
        "search_knowledge" => {
            let input: KnowledgeSearchInput = strict(arguments)?;
            optional_safe_id(input.project_id.as_deref())?;
            validate_search(input.query.as_deref())?;
            bounded_limit(input.limit)?;
            bounded_snippet(input.snippet_length)?;
            Invocation::read(
                "search_knowledge",
                "/api/workbench/knowledge/search".into(),
                "workspace",
                input.workspace_id,
                without_nulls(json!({
                    "query": input.query,
                    "projectId": input.project_id,
                    "status": input.status,
                    "limit": input.limit.unwrap_or(DEFAULT_LIST_LIMIT),
                    "snippetLength": input.snippet_length.unwrap_or(DEFAULT_SNIPPET_LENGTH)
                })),
            )
        }
        "get_knowledge_item" => {
            let input: KnowledgeInput = strict(arguments)?;
            let id = input.knowledge_id;
            Invocation::read(
                "get_knowledge_item",
                format!("/api/workbench/knowledge/{id}"),
                "knowledge",
                id,
                json!({}),
            )
        }
        "get_ai_execution_context" => {
            let input: AiExecutionInput = strict(arguments)?;
            let id = input.ai_execution_id;
            Invocation::read(
                "get_ai_execution_context",
                format!("/api/workbench/ai-executions/{id}"),
                "ai_execution",
                id,
                json!({}),
            )
        }
        "create_goal" => {
            let input: CreateGoalInput = strict(arguments)?;
            Invocation::write(
                "create_goal",
                "/api/workbench/goals",
                "workspace",
                input.workspace_id,
                None,
                without_nulls_deep(json!({
                    "title":input.title,"description":input.description,"domainId":input.domain_id,
                    "parentId":input.parent_id,"horizon":input.horizon.unwrap_or_else(|| DEFAULT_GOAL_HORIZON.into()),
                    "startsAt":input.starts_at,"endsAt":input.ends_at
                })),
            )
        }
        "create_project" => {
            let input: CreateProjectInput = strict(arguments)?;
            Invocation::write(
                "create_project",
                "/api/workbench/projects/write",
                "workspace",
                input.workspace_id,
                None,
                without_nulls_deep(
                    json!({"name":input.name,"purpose":input.purpose,"domainId":input.domain_id,
                    "goalId":input.goal_id,"color":input.color.unwrap_or_else(|| DEFAULT_PROJECT_COLOR.into())}),
                ),
            )
        }
        "create_action" => {
            let input: CreateActionInput = strict(arguments)?;
            optional_date(input.due_date.as_deref())?;
            optional_date(input.focus_date.as_deref())?;
            Invocation::write(
                "create_action",
                "/api/workbench/actions/write",
                "workspace",
                input.workspace_id,
                None,
                without_nulls_deep(json!({"operation":"create","value":{
                    "projectId":input.project_id,"parentId":input.parent_id,"title":input.title,"note":input.note,
                    "priority":input.priority.unwrap_or_else(|| DEFAULT_ACTION_PRIORITY.into()),
                    "dueDate":input.due_date,"focusDate":input.focus_date,"estimateMin":input.estimate_min
                }})),
            )
        }
        "update_action" => {
            let input: UpdateActionInput = strict(arguments)?;
            optional_date(input.due_date.as_deref())?;
            optional_date(input.focus_date.as_deref())?;
            Invocation::write(
                "update_action",
                "/api/workbench/actions/write",
                "action",
                input.action_id,
                Some(input.expected_version),
                without_nulls_deep(json!({"operation":"update","value":{
                    "title":input.title,"note":input.note,"priority":input.priority,"dueDate":input.due_date,
                    "focusDate":input.focus_date,"estimateMin":input.estimate_min
                }})),
            )
        }
        "update_action_status" => {
            let input: ActionStatusWriteInput = strict(arguments)?;
            one_of(&input.status, &["PENDING", "DOING", "BLOCKED", "DONE"])?;
            Invocation::write(
                "update_action_status",
                "/api/workbench/actions/status",
                "action",
                input.action_id,
                Some(input.expected_version),
                json!({"status":input.status}),
            )
        }
        "reorder_action_children" => {
            let input: ReorderActionsInput = strict(arguments)?;
            validate_versioned_ids(&input.children, 25)?;
            Invocation::write(
                "reorder_action_children",
                "/api/workbench/actions/reorder",
                "action",
                input.parent_action_id,
                Some(input.expected_version),
                json!({"children":input.children}),
            )
        }
        "set_today_focus" => {
            let input: FocusWriteInput = strict(arguments)?;
            optional_date(Some(&input.date))?;
            if let Some(mode) = &input.mode {
                one_of(mode, &["append", "replace"])?;
            }
            validate_versioned_ids(&input.actions, 5)?;
            if let Some(current) = &input.current {
                validate_versioned_ids(current, 5)?;
            }
            Invocation::write(
                "set_today_focus",
                "/api/workbench/focus",
                "workspace",
                input.workspace_id,
                Some(input.membership_version),
                without_nulls_deep(
                    json!({"date":input.date,"mode":input.mode.unwrap_or_else(|| DEFAULT_FOCUS_MODE.into()),
                    "actions":input.actions,"current":input.current.unwrap_or_default()}),
                ),
            )
        }
        "create_journal_entry" => {
            let input: JournalWriteInput = strict(arguments)?;
            Invocation::write(
                "create_journal_entry",
                "/api/workbench/journal",
                "workspace",
                input.workspace_id,
                None,
                without_nulls_deep(
                    json!({"title":input.title,"content":input.content,"mood":input.mood,"energy":input.energy,"entryDate":input.entry_date,"sensitivity":"normal"}),
                ),
            )
        }
        "create_daily_review" => {
            let input: DailyReviewWriteInput = strict(arguments)?;
            Invocation::write(
                "create_daily_review",
                "/api/workbench/reviews",
                "workspace",
                input.workspace_id,
                None,
                without_nulls_deep(
                    json!({"operation":"create_daily","title":input.title,"content":input.content,"happenedOn":input.happened_on}),
                ),
            )
        }
        "create_project_review" => {
            let input: ProjectReviewWriteInput = strict(arguments)?;
            let project_id = input.project_id;
            Invocation::write(
                "create_project_review",
                "/api/workbench/reviews",
                "project",
                project_id.clone(),
                None,
                without_nulls_deep(
                    json!({"operation":"create_project","projectId":project_id,"title":input.title,"content":input.content,"happenedOn":input.happened_on}),
                ),
            )
        }
        "apply_weekly_review" => {
            let input: WeeklyReviewWriteInput = strict(arguments)?;
            Invocation::write(
                "apply_weekly_review",
                "/api/workbench/reviews",
                "review",
                input.review_id,
                Some(input.expected_version),
                without_nulls_deep(
                    json!({"operation":"apply_weekly","title":input.title,"content":input.content,"happenedOn":input.happened_on}),
                ),
            )
        }
        "create_knowledge_item" => {
            let input: KnowledgeWriteInput = strict(arguments)?;
            Invocation::write(
                "create_knowledge_item",
                "/api/workbench/knowledge",
                "workspace",
                input.workspace_id,
                None,
                without_nulls_deep(json!({"projectId":input.project_id,"title":input.title,
                    "type":input.r#type.unwrap_or_else(|| DEFAULT_KNOWLEDGE_TYPE.into()),
                    "status":input.status.unwrap_or_else(|| DEFAULT_KNOWLEDGE_STATUS.into()),
                    "summary":input.summary,"content":input.content,"tags":input.tags.unwrap_or_default(),
                    "source":input.source})),
            )
        }
        "start_ai_execution" => {
            let input: StartAiExecutionInput = strict(arguments)?;
            let action_id = input.action_id;
            Invocation::write(
                "start_ai_execution",
                "/api/workbench/ai-executions",
                "action",
                action_id.clone(),
                None,
                without_nulls_deep(json!({"operation":"start","actionId":action_id,
                    "riskLevel":input.risk_level.unwrap_or_else(|| DEFAULT_AI_RISK_LEVEL.into()),
                    "actionType":input.action_type,"reason":input.reason,"plan":input.plan})),
            )
        }
        "append_ai_execution_output" => {
            let input: AppendAiOutputInput = strict(arguments)?;
            Invocation::write(
                "append_ai_execution_output",
                "/api/workbench/ai-executions",
                "ai_execution",
                input.ai_execution_id,
                Some(input.expected_version),
                without_nulls_deep(
                    json!({"operation":"append_output","type":input.r#type,"title":input.title,"content":input.content,
                    "data":input.data,"sourceUrls":input.source_urls.unwrap_or_default()}),
                ),
            )
        }
        "finish_ai_execution" => {
            let input: FinishAiExecutionInput = strict(arguments)?;
            Invocation::write(
                "finish_ai_execution",
                "/api/workbench/ai-executions",
                "ai_execution",
                input.ai_execution_id,
                Some(input.expected_version),
                without_nulls_deep(
                    json!({"operation":"finish","status":input.status,"error":input.error,
                    "blockReason":input.block_reason,"notificationSummary":input.notification_summary}),
                ),
            )
        }
        "preview_life_write" => {
            let input: PreviewLifeWriteInput = strict(arguments)?;
            if input.expected_version < 1
                || input.include_history == Some(true)
                || (input.include_history.is_some()
                    && !matches!(input.operation, HighRiskOperation::ExportKnowledge))
            {
                return Err(ToolInputError);
            }
            let resource_type = input.operation.resource_type();
            let resource_id = input.resource_id;
            let include_history =
                matches!(input.operation, HighRiskOperation::ExportKnowledge).then_some(false);
            Invocation::write(
                "preview_life_write",
                "/api/workbench/write-commands/preview",
                resource_type,
                resource_id,
                Some(input.expected_version),
                without_nulls_deep(json!({
                    "operation": input.operation,
                    "includeHistory": include_history
                })),
            )
        }
        "execute_confirmed_life_write" => {
            let _: EmptyInput = strict(arguments)?;
            Invocation::confirmed()
        }
        _ => Err(ToolInputError),
    }
}

fn strict<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, ToolInputError> {
    serde_json::from_value(value).map_err(|_| ToolInputError)
}

fn without_nulls(mut value: Value) -> Value {
    if let Value::Object(fields) = &mut value {
        fields.retain(|_, value| !value.is_null());
    }
    value
}

fn without_nulls_deep(mut value: Value) -> Value {
    match &mut value {
        Value::Object(fields) => {
            fields.retain(|_, value| !value.is_null());
            for value in fields.values_mut() {
                *value = without_nulls_deep(std::mem::take(value));
            }
        }
        Value::Array(items) => {
            for value in items {
                *value = without_nulls_deep(std::mem::take(value));
            }
        }
        _ => {}
    }
    value
}

fn validate_bounded_value(value: &Value, depth: usize) -> Result<(), ToolInputError> {
    if depth > 8 {
        return Err(ToolInputError);
    }
    match value {
        Value::String(value) if value.chars().count() > 30_000 => Err(ToolInputError),
        Value::Array(items) if items.len() > 25 => Err(ToolInputError),
        Value::Array(items) => items
            .iter()
            .try_for_each(|value| validate_bounded_value(value, depth + 1)),
        Value::Object(fields) if fields.len() > 64 => Err(ToolInputError),
        Value::Object(fields) => fields
            .values()
            .try_for_each(|value| validate_bounded_value(value, depth + 1)),
        _ => Ok(()),
    }
}

fn one_of(value: &str, allowed: &[&str]) -> Result<(), ToolInputError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(ToolInputError)
    }
}

fn validate_versioned_ids(values: &[VersionedId], maximum: usize) -> Result<(), ToolInputError> {
    if values.is_empty() || values.len() > maximum {
        return Err(ToolInputError);
    }
    let mut ids = std::collections::HashSet::new();
    for value in values {
        safe_id(&value.id)?;
        if value.expected_version < 1 || !ids.insert(&value.id) {
            return Err(ToolInputError);
        }
    }
    Ok(())
}

fn safe_id(value: &str) -> Result<(), ToolInputError> {
    if (1..=128).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b':')
        })
    {
        Ok(())
    } else {
        Err(ToolInputError)
    }
}

fn optional_safe_id(value: Option<&str>) -> Result<(), ToolInputError> {
    value.map_or(Ok(()), safe_id)
}

fn optional_date(value: Option<&str>) -> Result<(), ToolInputError> {
    match value {
        Some(value) => NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map(|_| ())
            .map_err(|_| ToolInputError),
        None => Ok(()),
    }
}

fn validate_window(from: Option<&str>, to: Option<&str>) -> Result<(), ToolInputError> {
    optional_date(from)?;
    optional_date(to)?;
    if let (Some(from), Some(to)) = (from, to) {
        let from = NaiveDate::parse_from_str(from, "%Y-%m-%d").map_err(|_| ToolInputError)?;
        let to = NaiveDate::parse_from_str(to, "%Y-%m-%d").map_err(|_| ToolInputError)?;
        let days = (to - from).num_days();
        if !(0..93).contains(&days) {
            return Err(ToolInputError);
        }
    }
    Ok(())
}

fn bounded_limit(value: Option<u32>) -> Result<(), ToolInputError> {
    if value.is_none_or(|value| (1..=100).contains(&value)) {
        Ok(())
    } else {
        Err(ToolInputError)
    }
}

fn bounded_snippet(value: Option<u32>) -> Result<(), ToolInputError> {
    if value.is_none_or(|value| (1..=1_000).contains(&value)) {
        Ok(())
    } else {
        Err(ToolInputError)
    }
}

fn validate_search(value: Option<&str>) -> Result<(), ToolInputError> {
    if value.is_none_or(|value| {
        let length = value.chars().count();
        (1..=200).contains(&length) && value.trim() == value && !value.chars().any(char::is_control)
    }) {
        Ok(())
    } else {
        Err(ToolInputError)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Life tool input is invalid")]
pub(crate) struct ToolInputError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_are_removed_before_hashing_and_routes_are_fixed() {
        let invocation = parse_invocation(
            "list_projects",
            json!({"workspaceId":"workspace-1","limit":25}),
        )
        .expect("valid invocation");
        assert_eq!(invocation.route, "/api/workbench/projects");
        let resource = invocation.resource.as_ref().expect("bound resource");
        assert_eq!(resource.resource_type, "workspace");
        assert_eq!(resource.id, "workspace-1");
        assert_eq!(invocation.api_input, json!({"archived":false,"limit":25}));
        assert_eq!(
            invocation.normalized_input_hash,
            normalized_input_hash(&json!({"archived":false,"limit":25})).expect("hash")
        );
    }

    #[test]
    fn api_defaults_are_materialized_before_hashing() {
        let cases = [
            (
                "list_projects",
                json!({"workspaceId":"workspace-1"}),
                json!({"archived":false,"limit":50}),
            ),
            (
                "list_actions",
                json!({"workspaceId":"workspace-1"}),
                json!({"limit":50}),
            ),
            (
                "search_journal",
                json!({"workspaceId":"workspace-1"}),
                json!({"limit":50,"snippetLength":500}),
            ),
            (
                "get_review_context",
                json!({"workspaceId":"workspace-1"}),
                json!({"limit":50,"snippetLength":500}),
            ),
            (
                "search_knowledge",
                json!({"workspaceId":"workspace-1"}),
                json!({"limit":50,"snippetLength":500}),
            ),
            (
                "create_goal",
                json!({"workspaceId":"workspace-1","title":"Goal"}),
                json!({"title":"Goal","horizon":"QUARTER"}),
            ),
            (
                "create_project",
                json!({"workspaceId":"workspace-1","name":"Project"}),
                json!({"name":"Project","color":"#197b70"}),
            ),
            (
                "create_action",
                json!({"workspaceId":"workspace-1","projectId":"project-1","title":"Action"}),
                json!({"operation":"create","value":{"projectId":"project-1","title":"Action","priority":"MEDIUM"}}),
            ),
            (
                "set_today_focus",
                json!({"workspaceId":"workspace-1","membershipVersion":1,"date":"2026-09-03","actions":[{"id":"action-1","expectedVersion":1}]}),
                json!({"date":"2026-09-03","mode":"append","actions":[{"id":"action-1","expectedVersion":1}],"current":[]}),
            ),
            (
                "create_journal_entry",
                json!({"workspaceId":"workspace-1","title":"Journal","content":"Entry"}),
                json!({"title":"Journal","content":"Entry","sensitivity":"normal"}),
            ),
            (
                "create_knowledge_item",
                json!({"workspaceId":"workspace-1","title":"Note","content":"Body"}),
                json!({"title":"Note","type":"NOTE","status":"APPROVED","content":"Body","tags":[]}),
            ),
            (
                "start_ai_execution",
                json!({"actionId":"action-1","actionType":"SUMMARY"}),
                json!({"operation":"start","actionId":"action-1","riskLevel":"LOW","actionType":"SUMMARY"}),
            ),
            (
                "append_ai_execution_output",
                json!({"aiExecutionId":"execution-1","expectedVersion":1,"type":"KNOWLEDGE","title":"Output"}),
                json!({"operation":"append_output","type":"KNOWLEDGE","title":"Output","sourceUrls":[]}),
            ),
            (
                "preview_life_write",
                json!({"operation":"export_knowledge","resourceId":"knowledge-1","expectedVersion":1}),
                json!({"operation":"export_knowledge","includeHistory":false}),
            ),
        ];

        for (tool, arguments, expected) in cases {
            let invocation = parse_invocation(tool, arguments).expect("valid invocation");
            assert_eq!(invocation.api_input, expected, "{tool} API input");
            assert_eq!(
                invocation.normalized_input_hash,
                normalized_input_hash(&expected).expect("hash"),
                "{tool} normalized hash"
            );
        }
    }

    #[test]
    fn arbitrary_fields_urls_queries_and_oversized_windows_are_rejected() {
        for arguments in [
            json!({"workspaceId":"workspace-1","url":"https://evil.test"}),
            json!({"workspaceId":"workspace-1","where":{"workspaceId":"other"}}),
            json!({"workspaceId":"workspace-1","sql":"select *"}),
        ] {
            assert!(parse_invocation("list_projects", arguments).is_err());
        }
        assert!(parse_invocation(
            "list_actions",
            json!({"workspaceId":"workspace-1","from":"2026-01-01","to":"2026-04-04"}),
        )
        .is_err());
        assert!(parse_invocation("run_sql", json!({})).is_err());
    }

    #[test]
    fn every_read_tool_maps_to_one_fixed_route_and_resource_type() {
        let cases = [
            (
                "get_today_context",
                json!({"workspaceId":"workspace-1"}),
                "/api/workbench/context/today",
                "workspace",
            ),
            (
                "get_system_overview",
                json!({"workspaceId":"workspace-1"}),
                "/api/workbench/context/system",
                "workspace",
            ),
            (
                "list_projects",
                json!({"workspaceId":"workspace-1"}),
                "/api/workbench/projects",
                "workspace",
            ),
            (
                "get_project_context",
                json!({"projectId":"project-1"}),
                "/api/workbench/projects/project-1",
                "project",
            ),
            (
                "list_actions",
                json!({"workspaceId":"workspace-1"}),
                "/api/workbench/actions",
                "workspace",
            ),
            (
                "get_action_detail",
                json!({"actionId":"action-1"}),
                "/api/workbench/actions/action-1",
                "action",
            ),
            (
                "search_journal",
                json!({"workspaceId":"workspace-1"}),
                "/api/workbench/journal/search",
                "workspace",
            ),
            (
                "get_review_context",
                json!({"workspaceId":"workspace-1"}),
                "/api/workbench/reviews/context",
                "workspace",
            ),
            (
                "get_weekly_review_context",
                json!({"workspaceId":"workspace-1"}),
                "/api/workbench/reviews/weekly",
                "workspace",
            ),
            (
                "search_knowledge",
                json!({"workspaceId":"workspace-1"}),
                "/api/workbench/knowledge/search",
                "workspace",
            ),
            (
                "get_knowledge_item",
                json!({"knowledgeId":"knowledge-1"}),
                "/api/workbench/knowledge/knowledge-1",
                "knowledge",
            ),
            (
                "get_ai_execution_context",
                json!({"aiExecutionId":"execution-1"}),
                "/api/workbench/ai-executions/execution-1",
                "ai_execution",
            ),
        ];
        for (tool, input, route, resource_type) in cases {
            let invocation = parse_invocation(tool, input).expect("valid fixed tool");
            assert_eq!(invocation.route, route);
            assert_eq!(
                invocation
                    .resource
                    .as_ref()
                    .expect("bound resource")
                    .resource_type,
                resource_type
            );
            assert_eq!(
                invocation.capability,
                catalog::tool(tool).expect("catalog").capability
            );
        }
    }

    #[test]
    fn versioned_write_tools_map_to_fixed_routes_and_strip_selectors() {
        let cases = [
            (
                "create_goal",
                json!({"workspaceId":"workspace-1","title":"Goal"}),
                "/api/workbench/goals",
                "workspace",
                None,
            ),
            (
                "update_action_status",
                json!({"actionId":"action-1","expectedVersion":7,"status":"DONE"}),
                "/api/workbench/actions/status",
                "action",
                Some(7),
            ),
            (
                "apply_weekly_review",
                json!({"reviewId":"review-1","expectedVersion":3,"content":"Weekly review"}),
                "/api/workbench/reviews",
                "review",
                Some(3),
            ),
        ];
        for (tool, input, route, resource_type, version) in cases {
            let invocation = parse_invocation(tool, input).expect("valid write");
            assert!(invocation.is_write);
            assert_eq!(invocation.route, route);
            let resource = invocation.resource.as_ref().expect("bound resource");
            assert_eq!(resource.resource_type, resource_type);
            assert_eq!(resource.expected_version, version);
            assert!(!invocation.api_input.to_string().contains("expectedVersion"));
            assert_eq!(
                invocation.capability,
                catalog::tool(tool).expect("catalog").capability
            );
        }
    }

    #[test]
    fn writes_reject_missing_versions_batches_and_high_risk_fields() {
        assert!(parse_invocation(
            "update_action",
            json!({"actionId":"action-1","title":"No version"})
        )
        .is_err());
        assert!(parse_invocation(
            "reorder_action_children",
            json!({"parentActionId":"action-1","expectedVersion":1,"children":[]})
        )
        .is_err());
        assert!(parse_invocation(
            "update_action",
            json!({"actionId":"action-1","expectedVersion":1,"delete":true})
        )
        .is_err());
        assert!(parse_invocation("execute_confirmed_life_write", json!({})).is_ok());
        assert!(parse_invocation(
            "execute_confirmed_life_write",
            json!({"writeCommandId":"attacker-controlled"})
        )
        .is_err());
    }

    #[test]
    fn high_risk_preview_and_confirmed_execution_are_fixed_and_separate() {
        let preview = parse_invocation(
            "preview_life_write",
            json!({
                "operation":"delete_action",
                "resourceId":"action-1",
                "expectedVersion":7
            }),
        )
        .expect("preview");
        assert_eq!(preview.capability, "write_command:preview");
        assert_eq!(preview.route, "/api/workbench/write-commands/preview");
        let resource = preview.resource.expect("preview resource");
        assert_eq!(resource.resource_type, "action");
        assert_eq!(resource.id, "action-1");
        assert_eq!(resource.expected_version, Some(7));
        assert_eq!(preview.api_input, json!({"operation":"delete_action"}));

        let confirmed = parse_invocation("execute_confirmed_life_write", json!({}))
            .expect("confirmed execution");
        assert_eq!(confirmed.capability, "write_command:execute");
        assert_eq!(confirmed.route, "/api/workbench/write-commands/execute");
        assert!(confirmed.resource.is_none());
        assert_eq!(confirmed.api_input, json!({}));

        assert!(parse_invocation(
            "preview_life_write",
            json!({
                "operation":"arbitrary_batch",
                "resourceId":"action-1",
                "expectedVersion":7
            })
        )
        .is_err());
    }
}
