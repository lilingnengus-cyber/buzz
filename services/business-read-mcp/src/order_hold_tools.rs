use super::*;
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OrderHoldInput {
    /// Exact sales order UUID from a scoped lookup; ask when the order is ambiguous.
    source_document_id: Uuid,
    /// Current source order version from a fresh detail read.
    #[schemars(range(min = 1))]
    expected_source_version: i64,
    /// Human-provided reason, at most 64 UTF-8 bytes; ask if missing.
    #[schemars(length(min = 1, max = 64))]
    reason: String,
}
#[tool_router(router=hold_router)]
impl BusinessReadMcp {
    #[tool(
        name = "prepare_sales_order_hold",
        description = "Pause creation and confirmation of shipments for a confirmed sales order. Inventory reservations remain unchanged. Use only on explicit human request. Read the exact order and current version first. Save an intent, show its effects and returned confirmation command, then wait for the human. Never report preparation as execution."
    )]
    async fn prepare_sales_order_hold(
        &self,
        Parameters(input): Parameters<OrderHoldInput>,
    ) -> Result<String, ErrorData> {
        Ok(self
            .invoke_write(
                "prepare_sales_order_hold",
                "sales_order_hold_intent:create",
                input,
            )
            .await)
    }
    #[tool(
        name = "approve_sales_order_hold",
        description = "Approve or reject only the order hold intent bound to the current human signed confirmation. No model-controlled business arguments. Never confirm for the human; report completion only when executed=true."
    )]
    async fn approve_sales_order_hold(&self) -> Result<String, ErrorData> {
        Ok(self
            .invoke_chat_approval(
                "approve_sales_order_hold",
                "sales_order_hold_intent:approve",
                "sales_order_hold_intent",
            )
            .await)
    }
    #[tool(
        name = "prepare_sales_order_release_hold",
        description = "Release a manual review hold on a confirmed sales order. This does not ship goods; later shipment checks still apply. Use only on explicit human request. Read the exact order and current version first. Save an intent, show its effects and returned confirmation command, then wait for the human. Never report preparation as execution."
    )]
    async fn prepare_sales_order_release_hold(
        &self,
        Parameters(input): Parameters<OrderHoldInput>,
    ) -> Result<String, ErrorData> {
        Ok(self
            .invoke_write(
                "prepare_sales_order_release_hold",
                "sales_order_release_hold_intent:create",
                input,
            )
            .await)
    }
    #[tool(
        name = "approve_sales_order_release_hold",
        description = "Approve or reject only the order hold intent bound to the current human signed confirmation. No model-controlled business arguments. Never confirm for the human; report completion only when executed=true."
    )]
    async fn approve_sales_order_release_hold(&self) -> Result<String, ErrorData> {
        Ok(self
            .invoke_chat_approval(
                "approve_sales_order_release_hold",
                "sales_order_release_hold_intent:approve",
                "sales_order_release_hold_intent",
            )
            .await)
    }
}

pub(super) fn router() -> ToolRouter<BusinessReadMcp> {
    BusinessReadMcp::hold_router()
}
