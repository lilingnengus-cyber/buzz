use super::*;

pub(super) async fn write_tool(
    State(state): State<ApiState>,
    Path(tool): Path<String>,
    request: Request<Body>,
) -> Response {
    if !WRITE_TOOLS.contains(&tool.as_str()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let is_approval = matches!(
        tool.as_str(),
        "approve_sales_order"
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
            | "approve_sales_order_cancellation"
            | "approve_purchase_order_cancellation"
            | "approve_inventory_opening"
    );
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
    if matches!(
        tool.as_str(),
        "prepare_receivable_allocation"
            | "prepare_payable_allocation"
            | "prepare_customer_receipt_reversal"
            | "prepare_supplier_payment_reversal"
            | "prepare_receivable_allocation_reversal"
            | "prepare_payable_allocation_reversal"
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
    match tool {
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
        "approve_sales_order"
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
    prepared["resourceRefs"] = json!([{"type":source_kind,"id":source_id,"title":"查看来源单据","bizUri":format!("biz://{}/{source_id}",source_kind.replace('_',"-"))}]);
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
mod allocation_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[tokio::test]
    async fn target_scope_is_checked_before_persisting_allocation_intent() {
        for allowed in [false, true] {
            for (tool, kind, category) in [
                (
                    "prepare_sales_order_cancellation",
                    "sales_order_cancellation_intent",
                    "order-cancellation",
                ),
                (
                    "prepare_purchase_order_cancellation",
                    "purchase_order_cancellation_intent",
                    "order-cancellation",
                ),
                (
                    "prepare_receivable_allocation",
                    "receivable_allocation_intent",
                    "allocation",
                ),
                (
                    "prepare_customer_receipt_reversal",
                    "customer_receipt_reversal_intent",
                    "reversal",
                ),
                (
                    "prepare_supplier_payment_reversal",
                    "supplier_payment_reversal_intent",
                    "reversal",
                ),
                (
                    "prepare_receivable_allocation_reversal",
                    "receivable_allocation_reversal_intent",
                    "reversal",
                ),
                (
                    "prepare_payable_allocation_reversal",
                    "payable_allocation_reversal_intent",
                    "reversal",
                ),
            ] {
                let writes = Arc::new(AtomicUsize::new(0));
                let snapshot = json!({"document":{"source":{"legalEntityId":"cn"},"allocations":[{"warehouseId":if allowed {"allowed"} else {"outside"}}]},"item":{"id":Uuid::new_v4(),"version":1}});
                let read = snapshot.clone();
                let counter = writes.clone();
                let server = Router::new()
                    .route(
                        &format!("/v1/agent-{category}-previews/{kind}"),
                        axum::routing::post(move || {
                            let value = read.clone();
                            async move { Json(value) }
                        }),
                    )
                    .route(
                        &format!("/v1/agent-{category}-intents/{kind}"),
                        axum::routing::post(move || {
                            let value = snapshot.clone();
                            let writes = counter.clone();
                            async move {
                                writes.fetch_add(1, Ordering::SeqCst);
                                Json(value)
                            }
                        }),
                    );
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let task = tokio::spawn(async move {
                    axum::serve(listener, server).await.unwrap();
                });
                let core = CoreClient {
                    client: reqwest::Client::new(),
                    base_url: Url::parse(&format!("http://{address}/")).unwrap(),
                    credential: "c".repeat(32),
                };
                let context = RequestContext {
                    enterprise_user_id: Uuid::new_v4(),
                    identity_binding_id: Uuid::new_v4(),
                    delegation_id: Uuid::new_v4(),
                    agent_id: "test".into(),
                    agent_turn_id: "test".into(),
                    trace_id: Uuid::new_v4(),
                    used_calls: 1,
                    required_scope: format!("{kind}:create"),
                    source_buzz_event_id: "a".repeat(64),
                    source_channel_id: "test".into(),
                };
                let grant = EffectiveGrant {
                    capability: business_iam::Capability::parse(&context.required_scope).unwrap(),
                    data_scope: DataScope::Restricted(BTreeMap::from([
                        ("legal_entity".into(), ["cn".into()].into()),
                        ("warehouse".into(), ["allowed".into()].into()),
                    ])),
                    obligations: Default::default(),
                };
                let response = forward_intent_prepare(
                    &core,
                    tool,
                    json!({"sourceDocumentId":Uuid::new_v4()}),
                    &context,
                    &grant,
                )
                .await;
                assert_eq!(
                    response.status(),
                    if allowed {
                        StatusCode::OK
                    } else {
                        StatusCode::FORBIDDEN
                    }
                );
                assert_eq!(writes.load(Ordering::SeqCst), usize::from(allowed));
                task.abort();
            }
        }
    }
}
