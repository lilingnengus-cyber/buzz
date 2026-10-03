use super::*;
pub(super) async fn forward_draft_write(
    core: &CoreClient,
    tool: &str,
    input: Value,
    context: &RequestContext,
) -> Response {
    let (endpoint, resource_type, uri_type) = match tool {
        "create_customer" => ("v1/agent-master-data/customers", "customer", "customer"),
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
    let Ok(url) = core.base_url.join(endpoint) else {
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
    let Some(id) = value.get("id").and_then(Value::as_str) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let expected_trace_id = context.trace_id.to_string();
    let expected_status = if tool == "create_customer" {
        "active"
    } else {
        "draft"
    };
    if value.get("status").and_then(Value::as_str) != Some(expected_status)
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
            "title": if tool == "create_customer" { "打开已创建的客户" } else { "打开已创建的业务草稿" },
            "bizUri": format!("biz://{uri_type}/{id}")
        }],
        "traceId": context.trace_id
    }))
    .into_response()
}
