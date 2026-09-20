use super::*;
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ReportType {
    ManagementProfitStatement,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReportFilters {
    /// Exact scoped customer UUIDs; supplied arrays must be nonempty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    customer_ids: Option<Vec<Uuid>>,
    /// Exact brand UUIDs; excludes unassigned brand facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    brand_ids: Option<Vec<Uuid>>,
    /// Exact business unit UUIDs; supplied arrays must be nonempty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    business_unit_ids: Option<Vec<Uuid>>,
    /// Exact warehouse UUIDs; excludes unassigned warehouse facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    warehouse_ids: Option<Vec<Uuid>>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReportSnapshotInput {
    /// Management profit statement; this is not a statutory financial statement.
    report_type: ReportType,
    /// Explicit reporting month in YYYY-MM format. Ask when missing or ambiguous.
    #[schemars(regex(pattern = r"^\d{4}-(0[1-9]|1[0-2])$"))]
    management_period: String,
    /// Reporting currency, for example CNY; never assume a different currency.
    #[schemars(regex(pattern = r"^[A-Z]{3}$"))]
    currency: String,
    /// Exact legal entity UUIDs from scoped lookup. Empty means the user's current scope.
    #[serde(default)]
    legal_entity_ids: Vec<Uuid>,
    /// Optional prior snapshot UUID explicitly being superseded; old snapshots remain immutable.
    supersedes_snapshot_id: Option<Uuid>,
    /// Optional explicit dimension subsets. Use scoped lookups; never guess UUIDs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    filters: Option<ReportFilters>,
}
#[tool_router(router=report_snapshot_router)]
impl BusinessReadMcp {
    #[tool(
        name = "prepare_management_report_snapshot",
        description = "Prepare an immutable management profit month-end snapshot only on explicit human request. Ask for missing month, currency or ambiguous legal entity. Show scope, amounts, data quality, whether an existing snapshot is reused, and the exact returned confirmation command. Wait for human confirmation; preparation has not generated a report. This is not a statutory financial statement."
    )]
    async fn prepare_management_report_snapshot(
        &self,
        Parameters(input): Parameters<ReportSnapshotInput>,
    ) -> Result<String, ErrorData> {
        Ok(self
            .invoke_write(
                "prepare_management_report_snapshot",
                "management_report_snapshot_intent:create",
                input,
            )
            .await)
    }
    #[tool(
        name = "approve_management_report_snapshot",
        description = "Approve or reject only the report snapshot intent bound to the current human signed confirmation. No model-controlled business arguments. Never confirm for the human. Report generation only when executed=true and return the verified report detail link."
    )]
    async fn approve_management_report_snapshot(&self) -> Result<String, ErrorData> {
        Ok(self
            .invoke_chat_approval(
                "approve_management_report_snapshot",
                "management_report_snapshot_intent:approve",
                "management_report_snapshot_intent",
            )
            .await)
    }
}
pub(super) fn router() -> ToolRouter<BusinessReadMcp> {
    BusinessReadMcp::report_snapshot_router()
}
