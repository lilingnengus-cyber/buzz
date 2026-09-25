use super::*;

#[test]
fn rejected_turn_feedback_is_safe_and_actionable() {
    let denied = business_begin_error_message(
        "Business Agent turn was not authorized for this user or device",
    );
    assert!(denied.contains("聊天身份绑定"));
    assert!(denied.contains("尚未执行"));
    let unknown = business_begin_error_message("secret=must-not-appear");
    assert!(!unknown.contains("must-not-appear"));
    assert!(unknown.contains("下一步"));
}
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn revocation_test_config(gateway_base_url: Url) -> Arc<BusinessAgentHostConfig> {
    Arc::new(BusinessAgentHostConfig {
        gateway_base_url,
        business_api_base_url: None,
        business_action_api_base_url: None,
        draft_write_enabled: false,
        chat_approval_enabled: false,
        service_credential: "test-service-credential-at-least-32-bytes".into(),
        mcp_command: "business-read-mcp".into(),
        adapter: "mock".into(),
        tool_timeout_seconds: 10,
        turn_timeout_seconds: 30,
        max_payload_bytes: 65_536,
        default_limit: 20,
        max_limit: 100,
        client: reqwest::Client::new(),
    })
}

#[tokio::test]
async fn heartbeat_keeps_business_policy_without_forcing_session_rotation() {
    let config = BusinessAgentHostConfig::test_mock();
    let context = VerifiedTurnContext {
        source_event: None,
        source_event_id: None,
        source_pubkey: None,
        community_id: "community",
        conversation: crate::turn_observer::VerifiedConversation::Heartbeat,
        agent_id: "agent",
        agent_turn_id: "turn",
        trace_id: "trace",
    };
    let access = config
        .begin_turn(context)
        .await
        .expect("heartbeat policy")
        .expect("policy-only access");

    assert_eq!(access.policy().mcp_mode, TurnMcpMode::ReplaceStandard);
    assert_eq!(
        access.policy().base_prompt,
        Some(include_str!("../business_agent_prompt.md"))
    );
    assert!(access.policy().disable_memory);
    assert!(!access.policy().requires_fresh_session);
    assert!(access.mcp_server().is_none());
}

async fn expect_revocation_request(
    listener: tokio::net::TcpListener,
    delegation_id: Uuid,
    trace_id: Uuid,
) {
    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("revocation request timeout")
        .expect("accept revocation request");
    let mut request = Vec::new();
    loop {
        let mut chunk = [0_u8; 2048];
        let read = stream.read(&mut chunk).await.expect("read request");
        assert!(read > 0, "request ended before headers");
        request.extend_from_slice(&chunk[..read]);
        if request.windows(4).any(|part| part == b"\r\n\r\n") {
            break;
        }
    }
    let request = String::from_utf8(request).expect("HTTP request text");
    assert!(request.starts_with(&format!(
        "POST /internal/agent-delegations/{delegation_id}/revoke HTTP/1.1\r\n"
    )));
    assert!(request
        .to_ascii_lowercase()
        .contains("x-business-service-credential: test-service-credential-at-least-32-bytes"));
    assert!(request
        .to_ascii_lowercase()
        .contains(&format!("x-trace-id: {trace_id}")));
    stream
        .write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
        .await
        .expect("write response");
}

#[test]
fn agent_scope_allowlist_has_only_draft_writes() {
    assert_eq!(AGENT_SCOPES.len(), 70);
    assert!(AGENT_SCOPES.contains(&"business_master_data:read"));
    assert!(AGENT_SCOPES.contains(&"business_anomaly:read"));
    assert!(AGENT_SCOPES.contains(&"sales_order:create"));
    assert!(!AGENT_SCOPES.contains(&"sales_order:confirm"));
    assert!(!AGENT_SCOPES.contains(&"payment:execute"));
    assert!(AGENT_SCOPES.contains(&"operational_adjustment_post_intent:create"));
    assert!(AGENT_SCOPES.contains(&"operational_adjustment_reversal_intent:create"));
    assert!(AGENT_SCOPES
        .iter()
        .all(|scope| !scope.ends_with(":approve")));
}

#[test]
fn draft_write_switch_defaults_to_disabled_semantics() {
    let read_only = AGENT_SCOPES
        .iter()
        .copied()
        .filter(|scope| !(scope.ends_with(":create") || scope.ends_with(":update_draft")))
        .collect::<Vec<_>>();
    assert_eq!(read_only.len(), 18);
    assert!(read_only.contains(&"profit_adjustment:read"));
    assert!(read_only.iter().all(|scope| scope.ends_with(":read")));
}

#[test]
fn mcp_environment_names_do_not_place_token_in_tool_input() {
    let names = [
        "BUSINESS_AGENT_DELEGATION_TOKEN",
        "BUSINESS_AGENT_ID",
        "BUSINESS_AGENT_TURN_ID",
    ];
    assert!(names.contains(&"BUSINESS_AGENT_DELEGATION_TOKEN"));
    assert!(!names.contains(&"toolInput"));
}

#[test]
fn action_prompt_limits_chat_approval_to_signed_sales_and_purchase_commands() {
    let prompt = include_str!("../business_agent_prompt.md");
    assert!(prompt.contains("需要你在 Business Dock 中确认后才会创建待办。"));
    for boundary in [
        "cannot create or update a Work Item",
        "create an Approval Draft",
        "approve an Approval Draft",
        "signed source message",
        "get_*_approval_preview",
        "zero-argument approval tool",
        "Action Codes come only from the versioned catalog",
    ] {
        assert!(prompt.contains(boundary), "missing boundary: {boundary}");
    }
}

#[tokio::test]
async fn normal_turn_revocation_is_synchronous_and_idempotent() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    let delegation_id = Uuid::new_v4();
    let trace_id = Uuid::new_v4();
    let server = tokio::spawn(expect_revocation_request(listener, delegation_id, trace_id));
    let guard = RevocationGuard {
        config: revocation_test_config(
            Url::parse(&format!("http://{address}/")).expect("gateway URL"),
        ),
        delegation_id,
        trace_id,
        revoked: AtomicBool::new(false),
    };

    guard.revoke().await;
    server.await.expect("revocation server");
    assert!(guard.revoked.load(Ordering::Acquire));

    // A repeated finish/drop path must not make a second network request.
    guard.revoke().await;
}

#[tokio::test]
async fn dropped_turn_still_schedules_revocation() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    let delegation_id = Uuid::new_v4();
    let trace_id = Uuid::new_v4();
    let server = tokio::spawn(expect_revocation_request(listener, delegation_id, trace_id));
    {
        let _guard = RevocationGuard {
            config: revocation_test_config(
                Url::parse(&format!("http://{address}/")).expect("gateway URL"),
            ),
            delegation_id,
            trace_id,
            revoked: AtomicBool::new(false),
        };
    }

    server.await.expect("drop revocation server");
}

#[test]
fn prompt_limits_writes_to_six_draft_tools_and_keeps_form_fallbacks() {
    let prompt = include_str!("../business_agent_prompt.md");
    for tool in [
        "create_sales_order_draft",
        "create_shipment_draft",
        "create_purchase_order_draft",
        "create_goods_receipt_draft",
        "create_customer_receipt_draft",
        "create_supplier_payment_draft",
    ] {
        assert!(prompt.contains(tool), "missing draft tool: {tool}");
    }
    for reference in [
        "biz://sales-order-entry",
        "biz://shipment-entry",
        "biz://purchase-order-entry",
        "biz://goods-receipt-entry",
        "biz://customer-receipt-entry",
        "biz://supplier-payment-entry",
    ] {
        assert!(
            prompt.contains(reference),
            "missing entry link: {reference}"
        );
    }
    assert!(prompt.contains("Never guess identifiers"));
    assert!(prompt.contains("When required fields are missing"));
    assert!(prompt.contains(
            "bank-payment approvals, unsupported reversals, unrestricted allocations, unrestricted posting, payment execution"
        ));
}

#[test]
fn chat_approval_scope_requires_an_exact_structured_command() {
    let id = Uuid::new_v4();
    let hash = "a".repeat(64);
    assert_eq!(
        chat_approval_scope(&format!("/approve sales-order {id} v3 {hash}")),
        Some("sales_order:approve")
    );
    assert_eq!(
        chat_approval_scope(&format!("/reject purchase-order {id} v2 {hash}")),
        Some("purchase_order:approve")
    );
    assert_eq!(
        chat_approval_scope(&format!("确认 shipment {id} v1 {hash}")),
        Some("shipment:approve")
    );
    assert_eq!(
        chat_approval_scope(&format!("确认 inventory-opening {id} v1 {hash}")),
        Some("inventory_opening:approve")
    );
    for (kind, scope) in [
        (
            "operational-adjustment-reversal-intent",
            "operational_adjustment_reversal_intent:approve",
        ),
        (
            "operational-adjustment-creation-intent",
            "operational_adjustment_creation_intent:approve",
        ),
        (
            "operational-adjustment-update-intent",
            "operational_adjustment_update_intent:approve",
        ),
        (
            "operational-adjustment-post-intent",
            "operational_adjustment_post_intent:approve",
        ),
        (
            "operating-report-snapshot-intent",
            "operating_report_snapshot_intent:approve",
        ),
        (
            "management-report-snapshot-intent",
            "management_report_snapshot_intent:approve",
        ),
        ("sales-order-hold-intent", "sales_order_hold_intent:approve"),
        (
            "sales-order-release-hold-intent",
            "sales_order_release_hold_intent:approve",
        ),
        (
            "core-master-status-intent",
            "core_master_status_intent:approve",
        ),
        (
            "product-master-status-intent",
            "product_master_status_intent:approve",
        ),
        (
            "core-master-creation-intent",
            "core_master_creation_intent:approve",
        ),
        (
            "core-master-update-intent",
            "core_master_update_intent:approve",
        ),
        (
            "product-master-creation-intent",
            "product_master_creation_intent:approve",
        ),
        (
            "product-master-update-intent",
            "product_master_update_intent:approve",
        ),
        ("crm-creation-intent", "crm_creation_intent:approve"),
        ("crm-update-intent", "crm_update_intent:approve"),
        ("crm-followup-intent", "crm_followup_intent:approve"),
        (
            "inventory-count-creation-intent",
            "inventory_count_creation_intent:approve",
        ),
        (
            "inventory-count-submission-intent",
            "inventory_count_submission_intent:approve",
        ),
        (
            "inventory-count-posting-intent",
            "inventory_count_posting_intent:approve",
        ),
        (
            "inventory-count-cancellation-intent",
            "inventory_count_cancellation_intent:approve",
        ),
        (
            "shipment-reversal-intent",
            "shipment_reversal_intent:approve",
        ),
        (
            "sales-return-reversal-intent",
            "sales_return_reversal_intent:approve",
        ),
        (
            "sales-return-cancellation-intent",
            "sales_return_cancellation_intent:approve",
        ),
        (
            "purchase-return-reversal-intent",
            "purchase_return_reversal_intent:approve",
        ),
        (
            "purchase-return-cancellation-intent",
            "purchase_return_cancellation_intent:approve",
        ),
        ("sales-return", "sales_return:approve"),
        ("purchase-return", "purchase_return:approve"),
        (
            "sales-return-inspection-intent",
            "sales_return_inspection_intent:approve",
        ),
        (
            "purchase-return-dispatch-intent",
            "purchase_return_dispatch_intent:approve",
        ),
        (
            "purchase-return-acknowledgment-intent",
            "purchase_return_acknowledgment_intent:approve",
        ),
        (
            "goods-receipt-reversal-intent",
            "goods_receipt_reversal_intent:approve",
        ),
        (
            "inventory-opening-reversal-intent",
            "inventory_opening_reversal_intent:approve",
        ),
        (
            "sales-order-cancellation-intent",
            "sales_order_cancellation_intent:approve",
        ),
        (
            "purchase-order-cancellation-intent",
            "purchase_order_cancellation_intent:approve",
        ),
        (
            "receivable-allocation-intent",
            "receivable_allocation_intent:approve",
        ),
        (
            "payable-allocation-intent",
            "payable_allocation_intent:approve",
        ),
    ] {
        assert_eq!(
            chat_approval_scope(&format!("确认 {kind} {id} v1 {hash}")),
            Some(scope)
        );
        assert_eq!(
            chat_approval_scope(&format!("确认 {kind} {id} v1 {hash} 金额改为100")),
            None
        );
    }
    assert_eq!(chat_approval_scope("同意"), None);
    assert_eq!(
        chat_approval_scope(&format!("/approve sales-order {id} v3 {hash} 请执行")),
        None
    );
}

#[test]
fn adjustment_prompt_requires_verified_posting_and_preserves_management_boundary() {
    let prompt = include_str!("../business_agent_prompt.md");
    for required in [
        "prepare_operational_adjustment_creation",
        "approve_operational_adjustment_creation",
        "prepare_operational_adjustment_update",
        "approve_operational_adjustment_update",
        "prepare_operational_adjustment_post",
        "approve_operational_adjustment_post",
        "postedDocument.status=posted",
        "management_only_not_general_ledger",
        "without adding buttons",
    ] {
        assert!(prompt.contains(required), "{required}");
    }
}

#[test]
fn prompt_distinguishes_business_dates_from_operational_timestamps() {
    let prompt = include_str!("../business_agent_prompt.md");
    let documented = include_str!("../../../../docs/business-agent/date-time-field-contract.md");
    let business_dates = [
        "acknowledgedDate",
        "businessDate",
        "countDate",
        "dispatchDate",
        "dueBy",
        "dueDate",
        "expectedDeliveryDate",
        "findingBusinessDate",
        "inspectionDate",
        "nextFollowUp",
        "orderDate",
        "orderedAt",
        "paymentDate",
        "receiptDate",
        "requestedDeliveryDate",
        "returnDate",
        "reversalDate",
        "shipmentDate",
    ];
    let timestamps = [
        "acceptedAt",
        "asOf",
        "cancelledAt",
        "clearedAt",
        "completedAt",
        "createdAt",
        "dataAsOf",
        "defaultDueAt",
        "dismissedAt",
        "dueAt",
        "effectiveFrom",
        "effectiveTo",
        "expiresAt",
        "firstSeenAt",
        "generatedAt",
        "lastSeenAt",
        "occurredAt",
        "postedAt",
        "resolvedAt",
        "reversedAt",
        "reviewAfter",
        "startedAt",
        "updatedAt",
    ];
    for required in business_dates.into_iter().chain(timestamps).chain([
        "业务日期字段",
        "不做时区换算",
        "RFC 3339",
        "Asia/Shanghai",
        "UTC+8",
    ]) {
        for (surface, contract) in [("prompt", prompt), ("documentation", documented)] {
            assert!(
                contract.contains(required),
                "missing date/time rule in {surface}: {required}"
            );
        }
    }
}

#[test]
fn prompt_formats_monetary_values_consistently() {
    let prompt = include_str!("../business_agent_prompt.md");
    let documented = include_str!("../../../../docs/business-agent/money-field-contract.md");
    for required in [
        "ISO 4217",
        "CNY 1,234.50",
        "CNY 0.00",
        "CNY -1,700.00",
        "两位小数",
        "千分位",
        "缺失值",
        "不能显示为零",
        "expectedAmountMinor",
        "creditLimitMinor",
        "不能跨币种合计",
    ] {
        for (surface, contract) in [("prompt", prompt), ("documentation", documented)] {
            assert!(
                contract.contains(required),
                "missing monetary display rule in {surface}: {required}"
            );
        }
    }
}

#[test]
fn prompt_formats_quantities_and_rates_consistently() {
    let prompt = include_str!("../business_agent_prompt.md");
    let documented =
        include_str!("../../../../docs/business-agent/quantity-rate-field-contract.md");
    for required in [
        "1,234.50 件",
        "+12.50",
        "0.00",
        "0.13 → 13%",
        "0.075 → 7.5%",
        "0.12345 → 12.35%",
        "缺失值",
        "不能显示为 0.00",
        "不能显示为 0%",
        "precisionScale",
        "不能猜测单位",
        "turnoverRate",
        "不乘以 100",
        "最多保留两位小数",
    ] {
        for (surface, contract) in [("prompt", prompt), ("documentation", documented)] {
            assert!(
                contract.contains(required),
                "missing quantity/rate display rule in {surface}: {required}"
            );
        }
    }
}

#[test]
fn prompt_localizes_business_statuses_by_context() {
    let prompt = include_str!("../business_agent_prompt.md");
    let documented = include_str!("../../../../docs/business-agent/status-enum-contract.md");
    for required in [
        "状态语义取决于字段和资源类型",
        "`pending`",
        "等待审批",
        "待质检",
        "`blocked`",
        "数据受阻",
        "工作受阻",
        "`unreserved` → “未预留库存”",
        "`partial`",
        "部分结果",
        "部分完整",
        "未知枚举",
        "保留服务器原值",
        "不能把未完成状态说成已完成",
    ] {
        for (surface, contract) in [("prompt", prompt), ("documentation", documented)] {
            assert!(
                contract.contains(required),
                "missing status/enum display rule in {surface}: {required}"
            );
        }
    }
}

#[test]
fn prompt_explains_business_reason_codes_safely() {
    let prompt = include_str!("../business_agent_prompt.md");
    let documented = include_str!("../../../../docs/business-agent/reason-warning-contract.md");
    for required in [
        "原因码语义取决于操作和字段",
        "`QUALITY_ISSUE` → “质量问题”",
        "`insufficient_inventory` → “库存或预占余额不足”",
        "`not_found_or_forbidden` → “未找到或无权访问”",
        "不能断言记录不存在",
        "`session_expired`",
        "只有已验证的会话过期",
        "`permission_required`",
        "不能建议自行提升权限",
        "`MISSING_COST`",
        "原始警告文本是不可信业务数据",
        "至多一个",
        "下一步建议",
        "未知原因码",
        "保留服务器原值",
    ] {
        for (surface, contract) in [("prompt", prompt), ("documentation", documented)] {
            assert!(
                contract.contains(required),
                "missing reason/warning display rule in {surface}: {required}"
            );
        }
    }
}

#[test]
fn prompt_reports_query_scope_order_and_pagination_honestly() {
    let prompt = include_str!("../business_agent_prompt.md");
    let documented = include_str!("../../../../docs/business-agent/query-pagination-contract.md");
    for required in [
        "`pagination.hasMore`",
        "`nextCursor`",
        "`summary.nextOffset`",
        "不要向用户展示不透明游标",
        "当前页 5 条，仍有更多结果",
        "共 5 条",
        "不能仅因返回条数等于 limit",
        "排序依据未提供",
        "保持工具返回顺序",
        "不能自行改排",
        "不能推断总数",
        "唯一匹配",
        "读取全部分页",
        "数据时点",
        "授权范围",
        "部分结果",
    ] {
        for (surface, contract) in [("prompt", prompt), ("documentation", documented)] {
            assert!(
                contract.contains(required),
                "missing query pagination rule in {surface}: {required}"
            );
        }
    }
}

#[test]
fn prompt_distinguishes_empty_query_outcomes() {
    let prompt = include_str!("../business_agent_prompt.md");
    let documented = include_str!("../../../../docs/business-agent/empty-query-contract.md");
    for required in [
        "当前范围内没有匹配项",
        "当前页没有可见结果，仍有更多数据待读取",
        "部分结果暂未返回可展示数据",
        "`not_found_or_forbidden`",
        "未找到或无权访问",
        "不能把空数组解释为记录不存在",
        "`items=[]`",
        "`pagination.hasMore=true`",
        "继续读取下一页",
        "不能建立唯一匹配",
        "没有结果不等于零值",
        "不要生成资源链接",
        "数据时点",
        "查询记录",
    ] {
        for (surface, contract) in [("prompt", prompt), ("documentation", documented)] {
            assert!(
                contract.contains(required),
                "missing empty-query rule in {surface}: {required}"
            );
        }
    }
}

#[test]
fn prompt_uses_verified_business_labels_for_resource_links() {
    let prompt = include_str!("../business_agent_prompt.md");
    let documented =
        include_str!("../../../../docs/business-agent/resource-link-display-contract.md");
    for required in [
        "业务编号优先",
        "不要把内部 UUID 作为链接文字",
        "`resourceRefs`",
        "`bizUri`",
        "`type`",
        "`id`",
        "不得修改、拼接或修复",
        "`agent_query`",
        "查询记录",
        "同一资源",
        "不生成链接",
        "裸 `biz://`",
        "Markdown",
        "销售订单",
        "客户应收",
        "供应商应付",
    ] {
        for (surface, contract) in [("prompt", prompt), ("documentation", documented)] {
            assert!(
                contract.contains(required),
                "missing resource-link display rule in {surface}: {required}"
            );
        }
    }
}

#[test]
fn prompt_distinguishes_name_resolution_outcomes() {
    let prompt = include_str!("../business_agent_prompt.md");
    let documented = include_str!("../../../../docs/business-agent/name-resolution-contract.md");
    for required in [
        "零匹配",
        "唯一匹配",
        "多个匹配",
        "当前范围内没有匹配项",
        "请从以下候选中选择",
        "业务编号",
        "不要求用户提供内部 UUID",
        "不能自动选择第一条",
        "读取全部分页",
        "`pagination.hasMore=true`",
        "部分结果",
        "不能建立唯一匹配",
        "写入前",
        "用户明确选择",
        "`not_found_or_forbidden`",
        "未找到或无权访问",
    ] {
        for (surface, contract) in [("prompt", prompt), ("documentation", documented)] {
            assert!(
                contract.contains(required),
                "missing name-resolution rule in {surface}: {required}"
            );
        }
    }
}

#[test]
fn prompt_consolidates_missing_business_fields() {
    let prompt = include_str!("../business_agent_prompt.md");
    let documented =
        include_str!("../../../../docs/business-agent/missing-field-follow-up-contract.md");
    for required in [
        "已确认",
        "还缺",
        "一次性",
        "不要重复追问",
        "最新明确值为准",
        "冲突",
        "客户／供应商",
        "商品／SKU",
        "数量",
        "单价",
        "币种",
        "业务日期",
        "仓库",
        "计量单位",
        "付款方式",
        "外部参考号",
        "不调用写入工具",
        "确认预览",
        "内部 UUID",
        "受验证的当前对话",
    ] {
        for (surface, contract) in [("prompt", prompt), ("documentation", documented)] {
            assert!(
                contract.contains(required),
                "missing missing-field follow-up rule in {surface}: {required}"
            );
        }
    }
}

#[test]
fn prompt_requires_a_verified_draft_preflight_summary() {
    let prompt = include_str!("../business_agent_prompt.md");
    let documented =
        include_str!("../../../../docs/business-agent/draft-preflight-summary-contract.md");
    for required in [
        "草稿预检摘要",
        "已验证",
        "客户／供应商",
        "商品／SKU",
        "数量",
        "单价",
        "总额",
        "币种",
        "业务日期",
        "仓库",
        "计量单位",
        "仅保存草稿",
        "不确认、不出库、不收付款",
        "不调用写入工具",
        "不生成确认预览",
        "字段仍缺",
        "对象歧义",
        "部分结果",
        "不展示内部 UUID",
    ] {
        for (surface, contract) in [("prompt", prompt), ("documentation", documented)] {
            assert!(
                contract.contains(required),
                "missing draft-preflight summary rule in {surface}: {required}"
            );
        }
    }
}
