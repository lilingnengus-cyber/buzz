use chrono::NaiveDate;
use life_workbench_contracts::{catalog, normalized_input_hash};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceContext {
    #[serde(rename = "type")]
    pub(crate) resource_type: String,
    pub(crate) id: String,
    pub(crate) expected_version: Option<i64>,
}

#[derive(Clone, Debug)]
pub(crate) struct Invocation {
    pub(crate) tool: &'static str,
    pub(crate) capability: &'static str,
    pub(crate) route: String,
    pub(crate) resource: ResourceContext,
    pub(crate) api_input: Value,
    pub(crate) normalized_input_hash: String,
    pub(crate) idempotency_key: Uuid,
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
            resource: ResourceContext {
                resource_type: resource_type.to_owned(),
                id: resource_id,
                expected_version: None,
            },
            normalized_input_hash: normalized_input_hash(&api_input).map_err(|_| ToolInputError)?,
            api_input,
            idempotency_key: Uuid::new_v4(),
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
                without_nulls(json!({"archived": input.archived, "limit": input.limit})),
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
                    "limit": input.limit
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
                    "limit": input.limit,
                    "snippetLength": input.snippet_length
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
                    "limit": input.limit,
                    "snippetLength": input.snippet_length
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
                    "limit": input.limit,
                    "snippetLength": input.snippet_length
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
        assert_eq!(invocation.resource.resource_type, "workspace");
        assert_eq!(invocation.resource.id, "workspace-1");
        assert_eq!(invocation.api_input, json!({"limit":25}));
        assert_eq!(
            invocation.normalized_input_hash,
            normalized_input_hash(&json!({"limit":25})).expect("hash")
        );
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
            assert_eq!(invocation.resource.resource_type, resource_type);
            assert_eq!(
                invocation.capability,
                catalog::tool(tool).expect("catalog").capability
            );
        }
    }
}
