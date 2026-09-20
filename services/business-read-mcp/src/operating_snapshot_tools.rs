use super::*;
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct OperatingSnapshotInput {
    /// Optional nonempty legal-entity UUID subset from scoped lookup. Never guess IDs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) legal_entity_ids: Option<Vec<Uuid>>,
    /// Daily or weekly reporting cadence; weekly periods start on Monday.
    #[schemars(regex(pattern = r"^(daily|weekly)$"))]
    pub(super) cadence: String,
    /// Explicit completed business period start date, YYYY-MM-DD.
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    pub(super) period_start: String,
    /// Reporting currency; ask if missing.
    #[schemars(regex(pattern = r"^[A-Z]{3}$"))]
    pub(super) currency: String,
    /// Explicit fixed UTC offset in minutes, for example 480 for UTC+8.
    #[schemars(range(min=-720,max=840))]
    pub(super) utc_offset_minutes: i16,
}
#[tool_router(router=operating_snapshot_router)]
impl BusinessReadMcp {
    #[tool(
        name = "prepare_operating_report_snapshot",
        description = "Prepare an immutable daily or weekly operating report only on explicit human request. Ask for missing cadence, completed period start, currency and fixed UTC offset. Weekly starts Monday. Show the requester's scope, metrics, quality, unavailable metrics and reasons (never describe unavailable as zero), inventory as of generation, and whether frozen content is reused. Display the exact returned confirmation command and wait for the human. Preparation does not generate a report. Mixed-domain dimension restrictions may be unsupported; never widen scope or retry without restrictions. Return only the verified detail link for an existing snapshot; a new preparation has no report link. This is not financial accounting."
    )]
    async fn prepare_operating_report_snapshot(
        &self,
        Parameters(input): Parameters<OperatingSnapshotInput>,
    ) -> Result<String, ErrorData> {
        Ok(self
            .invoke_write(
                "prepare_operating_report_snapshot",
                "operating_report_snapshot_intent:create",
                input,
            )
            .await)
    }
    #[tool(
        name = "approve_operating_report_snapshot",
        description = "Approve or reject only the operating report intent bound to the current human signed confirmation. No model-controlled business arguments. Never confirm for the human. Report generation only when executed=true; pending and rejected have no generated report. Preserve the requester and verified metrics. Return the verified detail link only for an executed result."
    )]
    async fn approve_operating_report_snapshot(&self) -> Result<String, ErrorData> {
        Ok(self
            .invoke_chat_approval(
                "approve_operating_report_snapshot",
                "operating_report_snapshot_intent:approve",
                "operating_report_snapshot_intent",
            )
            .await)
    }
}
pub(super) fn router() -> ToolRouter<BusinessReadMcp> {
    BusinessReadMcp::operating_snapshot_router()
}
