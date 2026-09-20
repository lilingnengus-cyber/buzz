use super::*;

pub(super) async fn core_read_result(
    core: &CoreClient,
    tool: &str,
    input: &Value,
    scope: &AuthorizationScope,
    context: &RequestContext,
) -> Response {
    if matches!(
        tool,
        "get_customer_receipt_allocations" | "get_supplier_payment_allocations"
    ) {
        return allocation_history::search(
            core,
            if tool == "get_customer_receipt_allocations" {
                "customer_receipt"
            } else {
                "supplier_payment"
            },
            input,
            scope,
            context,
        )
        .await;
    }
    if let Some((kind, mode)) = return_documents::family(tool) {
        return return_documents::read(core, kind, mode, input, scope, context).await;
    }
    if tool == "get_inventory_count_approval_preview" {
        return inventory_count_previews::read(core, input, scope, context).await;
    }
    if matches!(
        tool,
        "get_business_master_record" | "get_business_product_master_record"
    ) {
        return master_writes::read(core, input, scope, context).await;
    }
    if crm::handles(tool) {
        return crm::read(core, tool, input, scope, context).await;
    }
    if inventory_counts::handles(tool) {
        return inventory_counts::read(core, tool, input, scope, context).await;
    }
    let stock_kind = match tool {
        "search_shipments" => Some("shipment"),
        "search_goods_receipts" => Some("goods_receipt"),
        "search_inventory_openings" => Some("inventory_opening"),
        _ => None,
    };
    if let Some(kind) = stock_kind {
        return stock_documents::search(core, kind, input, scope, context).await;
    }
    let financial_kind = match tool {
        "search_customer_receipts" => Some("customer_receipt"),
        "search_supplier_payments" => Some("supplier_payment"),
        "search_receivables" => Some("receivable"),
        "search_payables" => Some("payable"),
        _ => None,
    };
    if let Some(kind) = financial_kind {
        return financial_documents::search(core, kind, input, scope, context).await;
    }
    if tool == "search_business_master_data" {
        return master_data::search(core, input, scope, context).await;
    }
    let endpoint = match tool {
        "get_sales_order" | "search_sales_orders" => "v1/sales-orders",
        "query_inventory_balance" => "v1/inventory-balances",
        "query_receivables" => "v1/trade-receivables",
        "get_purchase_order" | "search_purchase_orders" => "v1/purchase-orders",
        "query_payables" => "v1/trade-payables",
        "query_order_profit" => "v1/order-profits",
        "query_profitability_by_dimension" => "v1/profitability",
        "get_management_profit_report" => "v1/management-profit-report",
        "get_management_report_snapshot" => "v1/management-report-snapshots",
        "get_profit_evidence" => "v1/profit-evidence",
        "get_operating_dashboard" => "v1/operations/dashboard",
        "get_business_data_quality" => "v1/operations/data-quality",
        "get_sales_order_approval_preview"
        | "get_purchase_order_approval_preview"
        | "get_shipment_approval_preview"
        | "get_goods_receipt_approval_preview"
        | "get_customer_receipt_approval_preview"
        | "get_supplier_payment_approval_preview"
        | "get_inventory_opening_approval_preview" => "",
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let Ok(mut url) = core.base_url.join(endpoint) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if matches!(tool, "get_sales_order" | "get_purchase_order") {
        let Some(id) = input
            .get("orderId")
            .and_then(Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok())
        else {
            return (StatusCode::BAD_REQUEST, "invalid_filter").into_response();
        };
        let Ok(detail_url) = core.base_url.join(&format!(
            "v1/agent-documents/{}/{id}",
            if tool == "get_sales_order" {
                "sales-orders"
            } else {
                "purchase-orders"
            }
        )) else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        url = detail_url;
    }
    if matches!(
        tool,
        "get_sales_order_approval_preview"
            | "get_purchase_order_approval_preview"
            | "get_shipment_approval_preview"
            | "get_goods_receipt_approval_preview"
            | "get_customer_receipt_approval_preview"
            | "get_supplier_payment_approval_preview"
            | "get_inventory_opening_approval_preview"
    ) {
        let Some(id) = input
            .get("orderId")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
        else {
            return (StatusCode::BAD_REQUEST, "invalid_filter").into_response();
        };
        let kind = match tool {
            "get_sales_order_approval_preview" => "sales-orders",
            "get_purchase_order_approval_preview" => "purchase-orders",
            "get_shipment_approval_preview" => "stock/shipment",
            "get_goods_receipt_approval_preview" => "stock/goods_receipt",
            "get_customer_receipt_approval_preview" => "settlement/customer_receipt",
            "get_supplier_payment_approval_preview" => "settlement/supplier_payment",
            _ => "stock/inventory_opening",
        };
        let Ok(joined) = core
            .base_url
            .join(&format!("v1/agent-approval-previews/{kind}/{id}"))
        else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        url = joined;
    }
    if tool == "get_management_report_snapshot" {
        let Some(id) = input
            .get("snapshotId")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
        else {
            return (StatusCode::BAD_REQUEST, "invalid_filter").into_response();
        };
        let Ok(joined) = core
            .base_url
            .join(&format!("v1/management-report-snapshots/{id}"))
        else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        url = joined;
    } else if tool == "get_profit_evidence" {
        let Some(id) = input
            .get("orderId")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
        else {
            return (StatusCode::BAD_REQUEST, "invalid_filter").into_response();
        };
        let Ok(joined) = core.base_url.join(&format!("v1/profit-evidence/{id}")) else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        url = joined;
    }
    if matches!(
        tool,
        "query_order_profit"
            | "query_profitability_by_dimension"
            | "get_management_profit_report"
            | "get_operating_dashboard"
    ) {
        let mut query = url.query_pairs_mut();
        for (input_key, query_key) in [
            ("orderId", "orderId"),
            ("managementPeriod", "managementPeriod"),
            ("currency", "currency"),
            ("dimensionOne", "dimensionOne"),
            ("dimensionTwo", "dimensionTwo"),
            ("limit", "limit"),
        ] {
            if let Some(value) = input.get(input_key) {
                if let Some(value) = value.as_str() {
                    if input_key == "orderId" && Uuid::parse_str(value).is_err() {
                        query.append_pair("orderNumber", value);
                        continue;
                    }
                    query.append_pair(query_key, value);
                } else if let Some(value) = value.as_u64() {
                    query.append_pair(query_key, &value.to_string());
                }
            }
        }
    }
    let response = core
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
        .await;
    let Ok(response) = response else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !response.status().is_success() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let Ok(mut envelope) = response.json::<Value>().await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let approval_preview = matches!(
        tool,
        "get_sales_order_approval_preview"
            | "get_purchase_order_approval_preview"
            | "get_shipment_approval_preview"
            | "get_goods_receipt_approval_preview"
            | "get_customer_receipt_approval_preview"
            | "get_supplier_payment_approval_preview"
            | "get_inventory_opening_approval_preview"
    );
    if (approval_preview && !permits_document(&envelope["document"], scope))
        || (matches!(tool, "get_sales_order" | "get_purchase_order")
            && !permits_document(&envelope, scope))
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mut items = envelope
        .get_mut("items")
        .and_then(Value::as_array_mut)
        .map(std::mem::take)
        .unwrap_or_default();
    if approval_preview || matches!(tool, "get_sales_order" | "get_purchase_order") {
        items.push(envelope.clone());
    }
    if items.is_empty()
        && matches!(
            tool,
            "get_management_profit_report"
                | "get_operating_dashboard"
                | "get_business_data_quality"
        )
    {
        items.push(envelope.clone());
    } else if items.is_empty() && tool == "get_profit_evidence" {
        items = envelope
            .get_mut("facts")
            .and_then(Value::as_array_mut)
            .map(std::mem::take)
            .unwrap_or_default();
    }
    if let Some(exact) = input
        .get("orderId")
        .and_then(Value::as_str)
        .filter(|_| !approval_preview)
    {
        items.retain(|item| {
            item.get("id").and_then(Value::as_str) == Some(exact)
                || item.get("salesOrderId").and_then(Value::as_str) == Some(exact)
                || item.get("orderNumber").and_then(Value::as_str) == Some(exact)
                || item.get("purchaseOrderNumber").and_then(Value::as_str) == Some(exact)
        });
    }
    if tool == "query_order_profit" {
        let permits = |values: &std::collections::BTreeSet<String>, item: &Value, key: &str| {
            values.is_empty()
                || item
                    .get(key)
                    .and_then(Value::as_str)
                    .is_some_and(|value| values.contains(value))
        };
        items.retain(|item| {
            permits(&scope.legal_entity_ids, item, "legalEntityId")
                && permits(&scope.customer_ids, item, "customerId")
                && permits(&scope.brand_ids, item, "brandId")
                && permits(&scope.business_unit_ids, item, "businessUnitId")
        });
    }
    let exact = tool.starts_with("get_");
    if exact && items.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let limit = input
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .min(100) as usize;
    let has_more = items.len() > limit;
    items.truncate(limit);
    let refs = items
        .iter()
        .filter_map(|item| resource_ref(tool, item))
        .collect();
    let as_of = envelope
        .get("dataAsOf")
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&chrono::Utc))
        .unwrap_or_else(chrono::Utc::now);
    Json(BusinessToolResult {
        schema_version: 1,
        status: BusinessToolStatus::Ok,
        as_of,
        scope_summary: ScopeSummary {
            legal_entity_ids: scope.legal_entity_ids.iter().cloned().collect(),
            period: input
                .get("managementPeriod")
                .and_then(Value::as_str)
                .map(str::to_owned),
            currency: input
                .get("currency")
                .and_then(Value::as_str)
                .map(str::to_owned),
        },
        summary: BTreeMap::from([
            ("effectiveScopeHash".into(), json!(scope.hash())),
            (
                "source".into(),
                json!(if matches!(
                    tool,
                    "get_operating_dashboard" | "get_business_data_quality"
                ) {
                    "business-core-s1"
                } else if matches!(
                    tool,
                    "query_order_profit"
                        | "query_profitability_by_dimension"
                        | "get_management_profit_report"
                        | "get_management_report_snapshot"
                        | "get_profit_evidence"
                ) {
                    "business-core-b4"
                } else if tool.contains("purchase") || tool.contains("payable") {
                    "business-core-b3"
                } else {
                    "business-core-b2"
                }),
            ),
            (
                "ruleVersion".into(),
                envelope.get("ruleVersion").cloned().unwrap_or(Value::Null),
            ),
            (
                "sourceWatermark".into(),
                envelope
                    .get("sourceWatermark")
                    .cloned()
                    .unwrap_or(Value::Null),
            ),
        ]),
        items,
        pagination: Some(Pagination {
            next_cursor: has_more.then(|| "page:2".into()),
            has_more,
        }),
        resource_refs: refs,
        evidence: vec![Evidence {
            source_system: if matches!(
                tool,
                "get_operating_dashboard" | "get_business_data_quality"
            ) {
                "business-core-s1".into()
            } else if matches!(
                tool,
                "query_order_profit"
                    | "query_profitability_by_dimension"
                    | "get_management_profit_report"
                    | "get_management_report_snapshot"
                    | "get_profit_evidence"
            ) {
                "business-core-b4".into()
            } else if tool.contains("purchase") || tool.contains("payable") {
                "business-core-b3".into()
            } else {
                "business-core-b2".into()
            },
            object_type: tool.into(),
            object_id: "authoritative-postgresql".into(),
            version: Some(
                if matches!(
                    tool,
                    "get_operating_dashboard" | "get_business_data_quality"
                ) {
                    "S1".into()
                } else if matches!(
                    tool,
                    "query_order_profit"
                        | "query_profitability_by_dimension"
                        | "get_management_profit_report"
                        | "get_management_report_snapshot"
                        | "get_profit_evidence"
                ) {
                    "B4".into()
                } else if tool.contains("purchase") || tool.contains("payable") {
                    "B3".into()
                } else {
                    "B2".into()
                },
            ),
            updated_at: as_of,
        }],
        warnings: envelope
            .get("warnings")
            .and_then(Value::as_array)
            .map(|warnings| {
                warnings
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        trace_id: context.trace_id,
    })
    .into_response()
}

pub(super) fn resource_ref(tool: &str, item: &Value) -> Option<ResourceRef> {
    if tool == "get_operating_dashboard" {
        return Some(ResourceRef {
            r#type: "operations_dashboard".into(),
            id: None,
            title: "经营驾驶舱".into(),
            biz_uri: "biz://operations-dashboard".into(),
        });
    }
    if tool == "get_business_data_quality" {
        return Some(ResourceRef {
            r#type: "data_quality".into(),
            id: None,
            title: "业务数据质量".into(),
            biz_uri: "biz://data-quality".into(),
        });
    }
    if matches!(
        tool,
        "query_profitability_by_dimension" | "get_management_profit_report" | "get_profit_evidence"
    ) {
        return None;
    }
    let (kind, id, title) = if tool == "query_order_profit" {
        (
            "order-profit",
            item.get("salesOrderId")?.as_str()?,
            "订单真实利润",
        )
    } else if tool == "get_management_report_snapshot" {
        (
            "management-report",
            item.get("id")?.as_str()?,
            "管理利润报表快照",
        )
    } else if tool == "get_profit_evidence" {
        ("profit-evidence", item.get("id")?.as_str()?, "利润事实凭据")
    } else if tool == "query_profitability_by_dimension" {
        (
            "profitability",
            item.get("dimensionOneId")?.as_str()?,
            "盈利分析",
        )
    } else if tool == "get_management_profit_report" {
        (
            "management-report-current",
            item.get("managementPeriod")?.as_str()?,
            "当前管理利润报表",
        )
    } else if tool.contains("sales_order") {
        (
            "sales-order",
            item.get("id")?.as_str()?,
            item.get("orderNumber")?.as_str()?,
        )
    } else if tool.contains("purchase_order") {
        (
            "purchase-order",
            item.get("id")?.as_str()?,
            item.get("purchaseOrderNumber")?.as_str()?,
        )
    } else if tool.contains("inventory") {
        ("inventory", item.get("skuId")?.as_str()?, "库存台账")
    } else if tool.contains("payable") {
        ("supplier", item.get("supplierId")?.as_str()?, "供应商应付")
    } else {
        ("customer", item.get("customerId")?.as_str()?, "客户应收")
    };
    let biz_uri = if kind == "customer" {
        format!("biz://customer/{id}/receivables")
    } else if kind == "supplier" {
        format!("biz://supplier/{id}/payables")
    } else {
        format!("biz://{kind}/{id}")
    };
    Some(ResourceRef {
        r#type: kind.replace('-', "_"),
        id: Some(id.into()),
        title: title.into(),
        biz_uri,
    })
}
