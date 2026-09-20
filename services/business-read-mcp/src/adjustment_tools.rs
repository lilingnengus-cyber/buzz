use super::*;
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AdjustmentPostInput {
    /// Existing authorized adjustment batch UUID. Never guess identifiers.
    pub(super) batch_id: Uuid,
    /// Current batch version, obtained from verified data or supplied by the human.
    #[schemars(range(min = 1))]
    pub(super) expected_version: i64,
}
#[tool_router(router=adjustment_router)]
impl BusinessReadMcp {
    #[tool(
        name = "prepare_operational_adjustment_post",
        description = "Prepare posting of an existing operational expense/profit adjustment batch on explicit human request. Require the actual batch UUID and current version; ask for missing references and never guess. Show the amount, currency, allocation targets, scope and management-only accounting boundary from the verified preview. Preparation saves only an expiring intent and does not post. Present the exact returned confirmation/rejection command without buttons and wait for the human. Draft creation, replacement, reversal and discovery are not provided by this tool."
    )]
    async fn prepare_operational_adjustment_post(
        &self,
        Parameters(input): Parameters<AdjustmentPostInput>,
    ) -> Result<String, ErrorData> {
        Ok(self
            .invoke_write(
                "prepare_operational_adjustment_post",
                "operational_adjustment_post_intent:create",
                input,
            )
            .await)
    }
    #[tool(
        name = "approve_operational_adjustment_post",
        description = "Approve or reject only the operational adjustment posting intent bound to the current fresh signed human command. No model-controlled arguments. Never approve for the human. Report posting only when executed=true and a verified postedDocument is returned; pending/rejected do not post. This records management profit adjustments, not general-ledger entries or payments. Do not invent detail links."
    )]
    async fn approve_operational_adjustment_post(&self) -> Result<String, ErrorData> {
        Ok(self
            .invoke_chat_approval(
                "approve_operational_adjustment_post",
                "operational_adjustment_post_intent:approve",
                "operational_adjustment_post_intent",
            )
            .await)
    }
}
pub(super) fn router() -> ToolRouter<BusinessReadMcp> {
    BusinessReadMcp::adjustment_router()
}
