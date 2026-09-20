use super::*;

pub(super) async fn write_tool(
    State(state): State<ApiState>,
    Path(tool): Path<String>,
    request: Request<Body>,
) -> Response {
    if !WRITE_TOOLS.contains(&tool.as_str()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let is_approval = is_approval_tool(&tool);
    if (is_approval && !state.chat_approval_enabled) || (!is_approval && !state.draft_write_enabled)
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !authorized_service(
        request.headers(),
        &state.credential_hash,
        &state.service_audience,
    ) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(context) = parse_context(request.headers()) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let bytes = match axum::body::to_bytes(request.into_body(), state.max_payload_bytes).await {
        Ok(value) => value,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };
    let input: Value = match serde_json::from_slice(&bytes) {
        Ok(value) if valid_write_input(&tool, &value) => value,
        _ => return (StatusCode::BAD_REQUEST, "invalid_write_input").into_response(),
    };
    let Some(VerifiedAuthority::Iam(grant)) = state
        .verifier
        .verify_write(&context, is_approval.then_some(&input))
        .await
    else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Some(required) = required_capability(&tool) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if context.required_scope != required || grant.capability.as_str() != required {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(core) = state.core.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if adjustment_writes::family(&tool).is_some() {
        return adjustment_writes::forward(core, &tool, input, &context, &grant).await;
    }
    if operating_snapshot_writes::family(&tool).is_some() {
        return operating_snapshot_writes::forward(core, &tool, input, &context, &grant).await;
    }
    if report_snapshot_writes::family(&tool).is_some() {
        return report_snapshot_writes::forward(core, &tool, input, &context, &grant).await;
    }
    if order_hold_writes::family(&tool).is_some() {
        return order_hold_writes::forward(core, &tool, input, &context, &grant).await;
    }
    if master_writes::family(&tool).is_some() {
        return master_writes::forward(core, &tool, input, &context, &grant).await;
    }
    if crm_writes::family(&tool).is_some() {
        return crm_writes::forward(core, &tool, input, &context, &grant).await;
    }
    if inventory_count_writes::family(&tool).is_some() {
        return inventory_count_writes::forward(core, &tool, input, &context, &grant).await;
    }
    if matches!(
        tool.as_str(),
        "prepare_receivable_allocation"
            | "prepare_sales_return_reversal"
            | "prepare_purchase_return_reversal"
            | "prepare_sales_return_cancellation"
            | "prepare_purchase_return_cancellation"
            | "prepare_sales_return_inspection"
            | "prepare_purchase_return_dispatch"
            | "prepare_purchase_return_acknowledgment"
            | "prepare_payable_allocation"
            | "prepare_customer_receipt_reversal"
            | "prepare_supplier_payment_reversal"
            | "prepare_receivable_allocation_reversal"
            | "prepare_payable_allocation_reversal"
            | "prepare_shipment_reversal"
            | "prepare_goods_receipt_reversal"
            | "prepare_inventory_opening_reversal"
            | "prepare_sales_order_cancellation"
            | "prepare_purchase_order_cancellation"
    ) {
        return forward_intent_prepare(core, &tool, input, &context, &grant).await;
    }
    if !scope_allows_write(core, &tool, &input, &context, &grant).await {
        return StatusCode::FORBIDDEN.into_response();
    }
    if is_approval {
        forward_chat_approval(core, &tool, input, &context).await
    } else {
        forward_draft_write(core, &tool, input, &context).await
    }
}

pub(super) fn valid_write_input(tool: &str, input: &Value) -> bool {
    if adjustment_writes::family(tool).is_some() {
        return adjustment_writes::valid(tool, input);
    }
    if operating_snapshot_writes::family(tool).is_some() {
        return operating_snapshot_writes::valid(tool, input);
    }
    if report_snapshot_writes::family(tool).is_some() {
        return report_snapshot_writes::valid(tool, input);
    }
    if order_hold_writes::family(tool).is_some() {
        return order_hold_writes::valid(tool, input);
    }
    if master_writes::family(tool).is_some() {
        return master_writes::valid(tool, input);
    }
    if crm_writes::family(tool).is_some() {
        return crm_writes::valid(tool, input);
    }
    if inventory_count_writes::family(tool).is_some() {
        return inventory_count_writes::valid(tool, input);
    }
    match tool {
        "prepare_sales_return_reversal" | "prepare_purchase_return_reversal" => {
            serde_json::from_value::<
                business_core::document_approval::return_disposition::PrepareReturnDisposition,
            >(input.clone())
            .is_ok_and(|v| {
                serde_json::from_value::<business_core::b2::ReverseReturn>(v.command).is_ok()
            })
        }

        "prepare_sales_return_cancellation" | "prepare_purchase_return_cancellation" => {
            serde_json::from_value::<
                business_core::document_approval::return_disposition::PrepareReturnDisposition,
            >(input.clone())
            .is_ok_and(|v| {
                serde_json::from_value::<business_core::b2::CancelReturnDraft>(v.command).is_ok()
            })
        }

        "update_sales_return_draft" | "update_purchase_return_draft" => {
            serde_json::from_value::<UpdateReturnDraft>(input.clone())
                .is_ok_and(|v| v.draft.expected_version > 0 && v.draft.expected_source_version > 0)
        }
        "create_sales_return_draft" => {
            serde_json::from_value::<business_core::b2::CreateReturn>(input.clone())
                .is_ok_and(|v| v.expected_source_version.is_some_and(|v| v > 0))
        }
        "create_purchase_return_draft" => {
            serde_json::from_value::<business_core::b2::CreateReturn>(input.clone())
                .is_ok_and(|v| v.expected_source_version.is_some_and(|v| v > 0))
        }
        "prepare_sales_return_inspection" => serde_json::from_value::<
            business_core::document_approval::return_disposition::PrepareReturnDisposition,
        >(input.clone())
        .is_ok_and(|v| {
            serde_json::from_value::<business_core::b2::InspectSalesReturn>(v.command).is_ok()
        }),
        "prepare_purchase_return_dispatch" => serde_json::from_value::<
            business_core::document_approval::return_disposition::PrepareReturnDisposition,
        >(input.clone())
        .is_ok_and(|v| {
            serde_json::from_value::<business_core::b2::DispatchPurchaseReturn>(v.command).is_ok()
        }),
        "prepare_purchase_return_acknowledgment" => serde_json::from_value::<
            business_core::document_approval::return_disposition::PrepareReturnDisposition,
        >(input.clone())
        .is_ok_and(|v| {
            serde_json::from_value::<business_core::b2::AcknowledgePurchaseReturn>(v.command)
                .is_ok()
        }),
        "prepare_shipment_reversal"
        | "prepare_goods_receipt_reversal"
        | "prepare_inventory_opening_reversal" => serde_json::from_value::<
            business_core::document_approval::stock_reversal::PrepareStockReversal,
        >(input.clone())
        .is_ok(),
        "prepare_sales_order_cancellation" | "prepare_purchase_order_cancellation" => {
            serde_json::from_value::<
                business_core::document_approval::order_cancellation::PrepareOrderCancellation,
            >(input.clone())
            .is_ok()
        }
        "prepare_customer_receipt_reversal"
        | "prepare_supplier_payment_reversal"
        | "prepare_receivable_allocation_reversal"
        | "prepare_payable_allocation_reversal" => serde_json::from_value::<
            business_core::document_approval::reversal::PrepareReversal,
        >(input.clone())
        .is_ok(),
        "prepare_receivable_allocation" | "prepare_payable_allocation" => serde_json::from_value::<
            business_core::document_approval::allocation::PrepareAllocation,
        >(input.clone())
        .is_ok(),
        "update_sales_order_draft" => {
            serde_json::from_value::<UpdateSalesDraft>(input.clone()).is_ok()
        }
        "update_purchase_order_draft" => {
            serde_json::from_value::<UpdatePurchaseDraft>(input.clone()).is_ok()
        }
        "create_inventory_opening_draft" => serde_json::from_value::<
            business_core::b2::model::CreateInventoryOpening,
        >(input.clone())
        .is_ok(),
        "create_sales_order_draft" => {
            serde_json::from_value::<business_core::b2::model::CreateSalesOrder>(input.clone())
                .is_ok()
        }
        "create_shipment_draft" => {
            serde_json::from_value::<business_core::b2::model::CreateShipment>(input.clone())
                .is_ok()
        }
        "create_customer_receipt_draft" => {
            serde_json::from_value::<business_core::b2::model::CreateCustomerReceipt>(input.clone())
                .is_ok()
        }
        "create_purchase_order_draft" => {
            serde_json::from_value::<business_core::b3::model::CreatePurchaseOrder>(input.clone())
                .is_ok()
        }
        "create_goods_receipt_draft" => {
            serde_json::from_value::<business_core::b3::model::CreateGoodsReceipt>(input.clone())
                .is_ok()
        }
        "create_supplier_payment_draft" => {
            serde_json::from_value::<business_core::b3::model::CreateSupplierPayment>(input.clone())
                .is_ok()
        }
        "approve_sales_return_reversal"
        | "approve_purchase_return_reversal"
        | "approve_sales_return_cancellation"
        | "approve_purchase_return_cancellation"
        | "approve_sales_order"
        | "approve_sales_return"
        | "approve_purchase_return"
        | "approve_sales_return_inspection"
        | "approve_purchase_return_dispatch"
        | "approve_purchase_return_acknowledgment"
        | "approve_purchase_order"
        | "approve_shipment"
        | "approve_goods_receipt"
        | "approve_customer_receipt"
        | "approve_supplier_payment"
        | "approve_receivable_allocation"
        | "approve_payable_allocation"
        | "approve_customer_receipt_reversal"
        | "approve_supplier_payment_reversal"
        | "approve_receivable_allocation_reversal"
        | "approve_payable_allocation_reversal"
        | "approve_shipment_reversal"
        | "approve_goods_receipt_reversal"
        | "approve_inventory_opening_reversal"
        | "approve_sales_order_cancellation"
        | "approve_purchase_order_cancellation"
        | "approve_inventory_opening" => {
            serde_json::from_value::<ChatApprovalToolInput>(input.clone()).is_ok()
        }
        _ => false,
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChatApprovalToolInput {
    document_id: Uuid,
    expected_version: i64,
    preview_hash: String,
    decision: business_core::document_approval::ApprovalDecision,
}

async fn forward_chat_approval(
    core: &CoreClient,
    tool: &str,
    input: Value,
    context: &RequestContext,
) -> Response {
    let Ok(input) = serde_json::from_value::<ChatApprovalToolInput>(input) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let path = match tool {
        "approve_sales_return_reversal" => format!(
            "v1/agent-approvals/return-dispositions/sales_return_reversal_intent/{}",
            input.document_id
        ),
        "approve_sales_return_cancellation" => format!(
            "v1/agent-approvals/return-dispositions/sales_return_cancellation_intent/{}",
            input.document_id
        ),
        "approve_purchase_return_reversal" => format!(
            "v1/agent-approvals/return-dispositions/purchase_return_reversal_intent/{}",
            input.document_id
        ),
        "approve_purchase_return_cancellation" => format!(
            "v1/agent-approvals/return-dispositions/purchase_return_cancellation_intent/{}",
            input.document_id
        ),
        "approve_sales_return" => format!(
            "v1/agent-approvals/returns/sales_return/{}",
            input.document_id
        ),
        "approve_purchase_return" => format!(
            "v1/agent-approvals/returns/purchase_return/{}",
            input.document_id
        ),
        "approve_sales_return_inspection" => format!(
            "v1/agent-approvals/return-dispositions/sales_return_inspection_intent/{}",
            input.document_id
        ),
        "approve_purchase_return_dispatch" => format!(
            "v1/agent-approvals/return-dispositions/purchase_return_dispatch_intent/{}",
            input.document_id
        ),
        "approve_purchase_return_acknowledgment" => format!(
            "v1/agent-approvals/return-dispositions/purchase_return_acknowledgment_intent/{}",
            input.document_id
        ),
        "approve_shipment_reversal" => format!(
            "v1/agent-approvals/stock-reversals/shipment_reversal_intent/{}",
            input.document_id
        ),
        "approve_goods_receipt_reversal" => format!(
            "v1/agent-approvals/stock-reversals/goods_receipt_reversal_intent/{}",
            input.document_id
        ),
        "approve_inventory_opening_reversal" => format!(
            "v1/agent-approvals/stock-reversals/inventory_opening_reversal_intent/{}",
            input.document_id
        ),
        "approve_sales_order_cancellation" => format!(
            "v1/agent-approvals/order-cancellations/sales_order_cancellation_intent/{}",
            input.document_id
        ),
        "approve_purchase_order_cancellation" => format!(
            "v1/agent-approvals/order-cancellations/purchase_order_cancellation_intent/{}",
            input.document_id
        ),

        "approve_customer_receipt_reversal" => format!(
            "v1/agent-approvals/reversals/customer_receipt_reversal_intent/{}",
            input.document_id
        ),
        "approve_supplier_payment_reversal" => format!(
            "v1/agent-approvals/reversals/supplier_payment_reversal_intent/{}",
            input.document_id
        ),
        "approve_receivable_allocation_reversal" => format!(
            "v1/agent-approvals/reversals/receivable_allocation_reversal_intent/{}",
            input.document_id
        ),
        "approve_payable_allocation_reversal" => format!(
            "v1/agent-approvals/reversals/payable_allocation_reversal_intent/{}",
            input.document_id
        ),

        "approve_customer_receipt" => format!(
            "v1/agent-approvals/settlement/customer_receipt/{}",
            input.document_id
        ),
        "approve_supplier_payment" => format!(
            "v1/agent-approvals/settlement/supplier_payment/{}",
            input.document_id
        ),
        "approve_receivable_allocation" => format!(
            "v1/agent-approvals/allocations/receivable_allocation_intent/{}",
            input.document_id
        ),
        "approve_payable_allocation" => format!(
            "v1/agent-approvals/allocations/payable_allocation_intent/{}",
            input.document_id
        ),
        "approve_shipment" => format!("v1/agent-approvals/stock/shipment/{}", input.document_id),
        "approve_goods_receipt" => format!(
            "v1/agent-approvals/stock/goods_receipt/{}",
            input.document_id
        ),
        "approve_inventory_opening" => format!(
            "v1/agent-approvals/stock/inventory_opening/{}",
            input.document_id
        ),
        "approve_sales_order" => format!("v1/agent-approvals/sales-orders/{}", input.document_id),
        "approve_purchase_order" => {
            format!("v1/agent-approvals/purchase-orders/{}", input.document_id)
        }
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let Ok(url) = core.base_url.join(&path) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let response = core
        .client
        .post(url)
        .header("x-business-service-credential", &core.credential)
        .header("x-service-audience", "business-core")
        .header(
            "x-enterprise-user-id",
            context.enterprise_user_id.to_string(),
        )
        .header("x-trace-id", context.trace_id.to_string())
        .header(
            "idempotency-key",
            format!("agent:{}:{tool}", context.delegation_id),
        )
        .json(&json!({
            "expectedVersion": input.expected_version,
            "previewHash": input.preview_hash,
            "decision": input.decision,
            "sourceBuzzEventId": context.source_buzz_event_id,
            "sourceChannelId": context.source_channel_id,
        }))
        .send()
        .await;
    let Ok(response) = response else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let status = response.status();
    let Ok(value) = response.json::<Value>().await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    (status, Json(value)).into_response()
}

pub(super) async fn forward_draft_write(
    core: &CoreClient,
    tool: &str,
    input: Value,
    context: &RequestContext,
) -> Response {
    let (endpoint, resource_type, uri_type) = match tool {
        "create_sales_return_draft" | "update_sales_return_draft" => (
            "v1/agent-drafts/returns/sales_return",
            "sales_return",
            "sales-return",
        ),
        "create_purchase_return_draft" | "update_purchase_return_draft" => (
            "v1/agent-drafts/returns/purchase_return",
            "purchase_return",
            "purchase-return",
        ),
        "update_sales_order_draft" => {
            ("v1/agent-drafts/sales-orders", "sales_order", "sales-order")
        }
        "update_purchase_order_draft" => (
            "v1/agent-drafts/purchase-orders",
            "purchase_order",
            "purchase-order",
        ),
        "create_inventory_opening_draft" => (
            "v1/agent-drafts/inventory-openings",
            "inventory_opening",
            "inventory-opening",
        ),
        "create_sales_order_draft" => {
            ("v1/agent-drafts/sales-orders", "sales_order", "sales-order")
        }
        "create_shipment_draft" => ("v1/agent-drafts/shipments", "shipment", "shipment"),
        "create_purchase_order_draft" => (
            "v1/agent-drafts/purchase-orders",
            "purchase_order",
            "purchase-order",
        ),
        "create_goods_receipt_draft" => (
            "v1/agent-drafts/goods-receipts",
            "goods_receipt",
            "goods-receipt",
        ),
        "create_customer_receipt_draft" => (
            "v1/agent-drafts/customer-receipts",
            "customer_receipt",
            "customer-receipt",
        ),
        "create_supplier_payment_draft" => (
            "v1/agent-drafts/supplier-payments",
            "supplier_payment",
            "supplier-payment",
        ),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let (endpoint, input, method) = if tool.starts_with("update_") {
        let Some(id) = input
            .get("documentId")
            .and_then(Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok())
        else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        (
            format!("{endpoint}/{id}"),
            input["draft"].clone(),
            reqwest::Method::PUT,
        )
    } else {
        (endpoint.to_owned(), input, reqwest::Method::POST)
    };
    let Ok(url) = core.base_url.join(&endpoint) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let response = core
        .client
        .request(method, url)
        .header("x-business-service-credential", &core.credential)
        .header("x-service-audience", "business-core")
        .header(
            "x-enterprise-user-id",
            context.enterprise_user_id.to_string(),
        )
        .header("x-trace-id", context.trace_id.to_string())
        .header(
            "idempotency-key",
            format!("agent:{}:{tool}", context.delegation_id),
        )
        .json(&input)
        .send()
        .await;
    let Ok(response) = response else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let status = response.status();
    let Ok(value) = response.json::<Value>().await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if !status.is_success() {
        return (status, Json(value)).into_response();
    }
    let Some(id) = value.get("id").and_then(Value::as_str) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let expected_trace_id = context.trace_id.to_string();
    if value.get("status").and_then(Value::as_str) != Some("draft")
        || value.get("traceId").and_then(Value::as_str) != Some(expected_trace_id.as_str())
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    Json(json!({
        "schemaVersion": 1,
        "status": "ok",
        "item": value,
        "resourceRefs": [{
            "type": resource_type,
            "id": id,
            "title": "打开业务草稿",
            "bizUri": format!("biz://{uri_type}/{id}")
        }],
        "traceId": context.trace_id
    }))
    .into_response()
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateSalesDraft {
    #[serde(rename = "documentId")]
    _document_id: Uuid,
    #[serde(rename = "draft")]
    _draft: business_core::b2::model::ReplaceSalesOrderDraft,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdatePurchaseDraft {
    #[serde(rename = "documentId")]
    _document_id: Uuid,
    #[serde(rename = "draft")]
    _draft: business_core::b3::model::ReplacePurchaseOrderDraft,
}

async fn scope_allows_write(
    core: &CoreClient,
    tool: &str,
    input: &Value,
    context: &RequestContext,
    grant: &EffectiveGrant,
) -> bool {
    let Some(scope) = iam_authorization_scope(grant, &context.required_scope) else {
        return false;
    };
    let path = match tool {
        "approve_sales_return_reversal" => input["documentId"].as_str().map(|id|format!("v1/agent-approval-previews/return-dispositions/sales_return_reversal_intent/{id}")),
        "approve_sales_return_cancellation" => input["documentId"].as_str().map(|id|format!("v1/agent-approval-previews/return-dispositions/sales_return_cancellation_intent/{id}")),
        "approve_purchase_return_reversal" => input["documentId"].as_str().map(|id|format!("v1/agent-approval-previews/return-dispositions/purchase_return_reversal_intent/{id}")),
        "approve_purchase_return_cancellation" => input["documentId"].as_str().map(|id|format!("v1/agent-approval-previews/return-dispositions/purchase_return_cancellation_intent/{id}")),
        "update_sales_return_draft" => input["documentId"].as_str().map(|id|format!("v1/agent-return-edit-sources/sales_return/{id}")),
        "update_purchase_return_draft" => input["documentId"].as_str().map(|id|format!("v1/agent-return-edit-sources/purchase_return/{id}")),
        "create_sales_return_draft"=>input["sourceId"].as_str().map(|id|format!("v1/agent-return-sources/sales_return/{id}")),
        "create_purchase_return_draft"=>input["sourceId"].as_str().map(|id|format!("v1/agent-return-sources/purchase_return/{id}")),
        "approve_sales_return"=>input["documentId"].as_str().map(|id|format!("v1/agent-approval-previews/returns/sales_return/{id}")),
        "approve_purchase_return"=>input["documentId"].as_str().map(|id|format!("v1/agent-approval-previews/returns/purchase_return/{id}")),
        "approve_sales_return_inspection"=>input["documentId"].as_str().map(|id|format!("v1/agent-approval-previews/return-dispositions/sales_return_inspection_intent/{id}")),
        "approve_purchase_return_dispatch"=>input["documentId"].as_str().map(|id|format!("v1/agent-approval-previews/return-dispositions/purchase_return_dispatch_intent/{id}")),
        "approve_purchase_return_acknowledgment"=>input["documentId"].as_str().map(|id|format!("v1/agent-approval-previews/return-dispositions/purchase_return_acknowledgment_intent/{id}")),
        "approve_shipment_reversal"=>input["documentId"].as_str().map(|id|format!("v1/agent-approval-previews/stock-reversals/shipment_reversal_intent/{id}")),
        "approve_goods_receipt_reversal"=>input["documentId"].as_str().map(|id|format!("v1/agent-approval-previews/stock-reversals/goods_receipt_reversal_intent/{id}")),
        "approve_inventory_opening_reversal"=>input["documentId"].as_str().map(|id|format!("v1/agent-approval-previews/stock-reversals/inventory_opening_reversal_intent/{id}")),
        "approve_sales_order_cancellation"=>input["documentId"].as_str().map(|id|format!("v1/agent-approval-previews/order-cancellations/sales_order_cancellation_intent/{id}")),
        "approve_purchase_order_cancellation"=>input["documentId"].as_str().map(|id|format!("v1/agent-approval-previews/order-cancellations/purchase_order_cancellation_intent/{id}")),

        "approve_customer_receipt_reversal" => input["documentId"].as_str().map(|id| {
            format!("v1/agent-approval-previews/reversals/customer_receipt_reversal_intent/{id}")
        }),
        "approve_supplier_payment_reversal" => input["documentId"].as_str().map(|id| {
            format!("v1/agent-approval-previews/reversals/supplier_payment_reversal_intent/{id}")
        }),
        "approve_receivable_allocation_reversal" => input["documentId"].as_str().map(|id| {
            format!(
                "v1/agent-approval-previews/reversals/receivable_allocation_reversal_intent/{id}"
            )
        }),
        "approve_payable_allocation_reversal" => input["documentId"].as_str().map(|id| {
            format!("v1/agent-approval-previews/reversals/payable_allocation_reversal_intent/{id}")
        }),

        "update_sales_order_draft" => input["documentId"]
            .as_str()
            .map(|id| format!("v1/agent-documents/sales-orders/{id}")),
        "update_purchase_order_draft" => input["documentId"]
            .as_str()
            .map(|id| format!("v1/agent-documents/purchase-orders/{id}")),
        "create_shipment_draft" => input["salesOrderId"]
            .as_str()
            .map(|id| format!("v1/agent-documents/sales-orders/{id}")),
        "create_goods_receipt_draft" => input["purchaseOrderId"]
            .as_str()
            .map(|id| format!("v1/agent-documents/purchase-orders/{id}")),
        "approve_sales_order" => input["documentId"]
            .as_str()
            .map(|id| format!("v1/agent-approval-previews/sales-orders/{id}")),
        "approve_purchase_order" => input["documentId"]
            .as_str()
            .map(|id| format!("v1/agent-approval-previews/purchase-orders/{id}")),
        "approve_shipment" => input["documentId"]
            .as_str()
            .map(|id| format!("v1/agent-approval-previews/stock/shipment/{id}")),
        "approve_goods_receipt" => input["documentId"]
            .as_str()
            .map(|id| format!("v1/agent-approval-previews/stock/goods_receipt/{id}")),
        "approve_customer_receipt" => input["documentId"]
            .as_str()
            .map(|id| format!("v1/agent-approval-previews/settlement/customer_receipt/{id}")),
        "approve_supplier_payment" => input["documentId"]
            .as_str()
            .map(|id| format!("v1/agent-approval-previews/settlement/supplier_payment/{id}")),
        "approve_receivable_allocation" => input["documentId"].as_str().map(|id| {
            format!("v1/agent-approval-previews/allocations/receivable_allocation_intent/{id}")
        }),
        "approve_payable_allocation" => input["documentId"].as_str().map(|id| {
            format!("v1/agent-approval-previews/allocations/payable_allocation_intent/{id}")
        }),
        "approve_inventory_opening" => input["documentId"]
            .as_str()
            .map(|id| format!("v1/agent-approval-previews/stock/inventory_opening/{id}")),
        _ => None,
    };
    let mut target = Value::Null;
    if let Some(path) = path {
        let Ok(url) = core.base_url.join(&path) else {
            return false;
        };
        let Ok(response) = core
            .client
            .get(url)
            .header("x-business-service-credential", &core.credential)
            .header("x-service-audience", "business-core")
            .header(
                "x-enterprise-user-id",
                context.enterprise_user_id.to_string(),
            )
            .header("x-trace-id", context.trace_id.to_string())
            .send()
            .await
        else {
            return false;
        };
        if !response.status().is_success() {
            return false;
        }
        let Ok(value) = response.json::<Value>().await else {
            return false;
        };
        target = if tool.starts_with("approve_") {
            value["document"].clone()
        } else if matches!(
            tool,
            "create_sales_return_draft"
                | "create_purchase_return_draft"
                | "update_sales_return_draft"
                | "update_purchase_return_draft"
        ) {
            value["item"].clone()
        } else {
            value
        };
        if !permits_document(&target, &scope) {
            return false;
        }
    }
    if tool.starts_with("approve_") {
        return true;
    }
    let mut desired = if tool.starts_with("update_") {
        input["draft"].clone()
    } else {
        input.clone()
    };
    // Derived authority fields must come from the current parent, never from an arbitrary model id.
    for key in [
        "legalEntityId",
        "customerId",
        "supplierId",
        "businessUnitId",
        "brandId",
    ] {
        if desired.get(key).is_none() && target.get(key).is_some() {
            desired[key] = target[key].clone();
        }
    }
    if matches!(
        tool,
        "create_sales_return_draft"
            | "create_purchase_return_draft"
            | "update_sales_return_draft"
            | "update_purchase_return_draft"
    ) {
        // The authoritative source was checked above, including every source line.
        return true;
    }
    permits_document(&desired, &scope)
}

pub(super) fn permits_document(value: &Value, scope: &AuthorizationScope) -> bool {
    fn collect<'a>(value: &'a Value, key: &str, out: &mut Vec<&'a str>) {
        match value {
            Value::Object(map) => {
                if let Some(id) = map.get(key).and_then(Value::as_str) {
                    out.push(id);
                }
                for child in map.values() {
                    collect(child, key, out);
                }
            }
            Value::Array(items) => {
                for child in items {
                    collect(child, key, out);
                }
            }
            _ => {}
        }
    }
    [
        ("legalEntityId", &scope.legal_entity_ids),
        ("warehouseId", &scope.warehouse_ids),
        ("customerId", &scope.customer_ids),
        ("supplierId", &scope.supplier_ids),
        ("brandId", &scope.brand_ids),
        ("businessUnitId", &scope.business_unit_ids),
    ]
    .into_iter()
    .all(|(key, allowed)| {
        if allowed.is_empty() {
            return true;
        }
        let mut ids = Vec::new();
        collect(value, key, &mut ids);
        if key == "brandId" {
            collect(value, "currentBrandId", &mut ids);
        }
        !ids.is_empty() && ids.iter().all(|id| allowed.contains(*id))
    })
}

async fn forward_intent_prepare(
    core: &CoreClient,
    tool: &str,
    input: Value,
    context: &RequestContext,
    grant: &EffectiveGrant,
) -> Response {
    let (kind, category, source_kind) = match tool {
        "prepare_sales_return_reversal" => (
            "sales_return_reversal_intent",
            "return-disposition",
            "sales_return",
        ),
        "prepare_sales_return_cancellation" => (
            "sales_return_cancellation_intent",
            "return-disposition",
            "sales_return",
        ),
        "prepare_purchase_return_reversal" => (
            "purchase_return_reversal_intent",
            "return-disposition",
            "purchase_return",
        ),
        "prepare_purchase_return_cancellation" => (
            "purchase_return_cancellation_intent",
            "return-disposition",
            "purchase_return",
        ),
        "prepare_sales_return_inspection" => (
            "sales_return_inspection_intent",
            "return-disposition",
            "sales_return",
        ),
        "prepare_purchase_return_dispatch" => (
            "purchase_return_dispatch_intent",
            "return-disposition",
            "purchase_return",
        ),
        "prepare_purchase_return_acknowledgment" => (
            "purchase_return_acknowledgment_intent",
            "return-disposition",
            "purchase_return",
        ),
        "prepare_shipment_reversal" => ("shipment_reversal_intent", "stock-reversal", "shipment"),
        "prepare_goods_receipt_reversal" => (
            "goods_receipt_reversal_intent",
            "stock-reversal",
            "goods_receipt",
        ),
        "prepare_inventory_opening_reversal" => (
            "inventory_opening_reversal_intent",
            "stock-reversal",
            "inventory_opening",
        ),
        "prepare_sales_order_cancellation" => (
            "sales_order_cancellation_intent",
            "order-cancellation",
            "sales_order",
        ),
        "prepare_purchase_order_cancellation" => (
            "purchase_order_cancellation_intent",
            "order-cancellation",
            "purchase_order",
        ),

        "prepare_receivable_allocation" => (
            "receivable_allocation_intent",
            "allocation",
            "customer_receipt",
        ),
        "prepare_payable_allocation" => (
            "payable_allocation_intent",
            "allocation",
            "supplier_payment",
        ),
        "prepare_customer_receipt_reversal" => (
            "customer_receipt_reversal_intent",
            "reversal",
            "customer_receipt",
        ),
        "prepare_supplier_payment_reversal" => (
            "supplier_payment_reversal_intent",
            "reversal",
            "supplier_payment",
        ),
        "prepare_receivable_allocation_reversal" => (
            "receivable_allocation_reversal_intent",
            "reversal",
            "customer_receipt",
        ),
        "prepare_payable_allocation_reversal" => (
            "payable_allocation_reversal_intent",
            "reversal",
            "supplier_payment",
        ),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let Some(scope) = iam_authorization_scope(grant, &context.required_scope) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let mut prepared = Value::Null;
    for phase in [
        format!("agent-{category}-previews"),
        format!("agent-{category}-intents"),
    ] {
        let Ok(url) = core.base_url.join(&format!("v1/{phase}/{kind}")) else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let response = core
            .client
            .post(url)
            .header("x-business-service-credential", &core.credential)
            .header("x-service-audience", "business-core")
            .header(
                "x-enterprise-user-id",
                context.enterprise_user_id.to_string(),
            )
            .header("x-trace-id", context.trace_id.to_string())
            .header(
                "idempotency-key",
                format!("agent:{}:{tool}", context.delegation_id),
            )
            .json(&input)
            .send()
            .await;
        let Ok(response) = response else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let status = response.status();
        let Ok(value) = response.json::<Value>().await else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        if !status.is_success() {
            return (status, Json(value)).into_response();
        }
        if !permits_document(&value["document"], &scope) {
            return StatusCode::FORBIDDEN.into_response();
        }
        prepared = value;
    }
    let Some(source_id) = input["sourceDocumentId"].as_str() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    prepared["item"]["status"] = json!("draft");
    prepared["schemaVersion"] = json!(1);
    prepared["status"] = json!("ok");
    let (link_kind, link_id, title) = (
        source_kind,
        source_id,
        if category == "return-disposition" {
            "查看退货单"
        } else {
            "查看来源单据"
        },
    );
    prepared["resourceRefs"] = json!([{"type":link_kind.replace('-',"_"),"id":link_id,"title":title,"bizUri":format!("biz://{}/{link_id}",link_kind.replace('_',"-"))}]);
    Json(prepared).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mutation_scope_rejects_missing_dimensions_and_any_unauthorized_line() {
        let scope = AuthorizationScope {
            legal_entity_ids: ["cn".into()].into(),
            warehouse_ids: ["allowed".into()].into(),
            ..Default::default()
        };
        assert!(permits_document(
            &json!({"legalEntityId":"cn","lines":[{"warehouseId":"allowed"}]}),
            &scope
        ));
        assert!(!permits_document(&json!({"legalEntityId":"cn"}), &scope));
        assert!(!permits_document(
            &json!({"legalEntityId":"cn","lines":[{"warehouseId":"allowed"},{"warehouseId":"other"}]}),
            &scope
        ));
        assert!(!permits_document(
            &json!({"legalEntityId":"sg","lines":[{"warehouseId":"allowed"}]}),
            &scope
        ));
    }
}

#[cfg(test)]
mod allocation_tests;
#[cfg(test)]
mod return_tests;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateReturnDraft {
    #[serde(rename = "documentId")]
    _document_id: Uuid,
    draft: business_core::b2::ReplaceReturnDraft,
}
