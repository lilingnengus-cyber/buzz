pub(super) const READ_TOOLS: [&str; 47] = [
    "search_operational_adjustments",
    "get_operational_adjustment",
    "get_business_master_record",
    "get_business_product_master_record",
    "search_crm_opportunities",
    "get_crm_opportunity",
    "search_inventory_counts",
    "get_inventory_count",
    "get_inventory_count_approval_preview",
    "search_inventory_count_options",
    "search_sales_returns",
    "search_purchase_returns",
    "get_sales_return_source",
    "get_purchase_return_source",
    "get_sales_return_approval_preview",
    "get_purchase_return_approval_preview",
    "search_shipments",
    "search_goods_receipts",
    "search_inventory_openings",
    "get_customer_receipt_allocations",
    "get_supplier_payment_allocations",
    "search_business_master_data",
    "search_customer_receipts",
    "search_supplier_payments",
    "search_receivables",
    "search_payables",
    "get_sales_order",
    "search_sales_orders",
    "get_purchase_order",
    "search_purchase_orders",
    "query_inventory_balance",
    "query_receivables",
    "query_payables",
    "query_order_profit",
    "query_profitability_by_dimension",
    "get_management_profit_report",
    "get_management_report_snapshot",
    "get_profit_evidence",
    "get_operating_dashboard",
    "get_business_data_quality",
    "get_shipment_approval_preview",
    "get_goods_receipt_approval_preview",
    "get_inventory_opening_approval_preview",
    "get_customer_receipt_approval_preview",
    "get_supplier_payment_approval_preview",
    "get_sales_order_approval_preview",
    "get_purchase_order_approval_preview",
];
pub(super) const ANOMALY_TOOLS: [&str; 8] = [
    "search_business_anomalies",
    "get_business_anomaly",
    "analyze_order_profit_risks",
    "analyze_receivable_risks",
    "analyze_inventory_risks",
    "analyze_purchase_cost_risks",
    "analyze_cross_domain_risks",
    "explain_profit_change",
];
pub(super) const WRITE_TOOLS: [&str; 94] = [
    "prepare_operational_adjustment_post",
    "approve_operational_adjustment_post",
    "prepare_operating_report_snapshot",
    "approve_operating_report_snapshot",
    "prepare_management_report_snapshot",
    "approve_management_report_snapshot",
    "prepare_sales_order_hold",
    "approve_sales_order_hold",
    "prepare_sales_order_release_hold",
    "approve_sales_order_release_hold",
    "prepare_core_master_creation",
    "approve_core_master_creation",
    "prepare_core_master_status",
    "approve_core_master_status",
    "prepare_product_master_status",
    "approve_product_master_status",
    "prepare_core_master_update",
    "approve_core_master_update",
    "prepare_product_master_creation",
    "approve_product_master_creation",
    "prepare_product_master_update",
    "approve_product_master_update",
    "prepare_crm_creation",
    "approve_crm_creation",
    "prepare_crm_update",
    "approve_crm_update",
    "prepare_crm_followup",
    "approve_crm_followup",
    "prepare_inventory_count_creation",
    "approve_inventory_count_creation",
    "prepare_inventory_count_submission",
    "approve_inventory_count_submission",
    "prepare_inventory_count_posting",
    "approve_inventory_count_posting",
    "prepare_inventory_count_cancellation",
    "approve_inventory_count_cancellation",
    "prepare_sales_return_reversal",
    "prepare_sales_return_cancellation",
    "prepare_purchase_return_reversal",
    "prepare_purchase_return_cancellation",
    "approve_sales_return_reversal",
    "approve_sales_return_cancellation",
    "approve_purchase_return_reversal",
    "approve_purchase_return_cancellation",
    "update_sales_return_draft",
    "update_purchase_return_draft",
    "create_sales_return_draft",
    "create_purchase_return_draft",
    "prepare_sales_return_inspection",
    "prepare_purchase_return_dispatch",
    "prepare_purchase_return_acknowledgment",
    "approve_sales_return",
    "approve_purchase_return",
    "approve_sales_return_inspection",
    "approve_purchase_return_dispatch",
    "approve_purchase_return_acknowledgment",
    "prepare_shipment_reversal",
    "approve_shipment_reversal",
    "prepare_goods_receipt_reversal",
    "approve_goods_receipt_reversal",
    "prepare_inventory_opening_reversal",
    "approve_inventory_opening_reversal",
    "prepare_sales_order_cancellation",
    "prepare_purchase_order_cancellation",
    "approve_sales_order_cancellation",
    "approve_purchase_order_cancellation",
    "prepare_customer_receipt_reversal",
    "prepare_supplier_payment_reversal",
    "prepare_receivable_allocation_reversal",
    "prepare_payable_allocation_reversal",
    "approve_customer_receipt_reversal",
    "approve_supplier_payment_reversal",
    "approve_receivable_allocation_reversal",
    "approve_payable_allocation_reversal",
    "prepare_receivable_allocation",
    "approve_receivable_allocation",
    "prepare_payable_allocation",
    "approve_payable_allocation",
    "update_sales_order_draft",
    "update_purchase_order_draft",
    "create_inventory_opening_draft",
    "create_sales_order_draft",
    "create_shipment_draft",
    "create_purchase_order_draft",
    "create_goods_receipt_draft",
    "create_customer_receipt_draft",
    "create_supplier_payment_draft",
    "approve_shipment",
    "approve_goods_receipt",
    "approve_inventory_opening",
    "approve_customer_receipt",
    "approve_supplier_payment",
    "approve_sales_order",
    "approve_purchase_order",
];

pub(super) fn required_capability(tool: &str) -> Option<&'static str> {
    match tool {
        "search_operational_adjustments" | "get_operational_adjustment" => {
            Some("profit_adjustment:read")
        }
        "prepare_operational_adjustment_post" => Some("operational_adjustment_post_intent:create"),
        "approve_operational_adjustment_post" => Some("operational_adjustment_post_intent:approve"),
        "prepare_operating_report_snapshot" => Some("operating_report_snapshot_intent:create"),
        "approve_operating_report_snapshot" => Some("operating_report_snapshot_intent:approve"),
        "prepare_management_report_snapshot" => Some("management_report_snapshot_intent:create"),
        "approve_management_report_snapshot" => Some("management_report_snapshot_intent:approve"),
        "prepare_sales_order_hold" => Some("sales_order_hold_intent:create"),
        "approve_sales_order_hold" => Some("sales_order_hold_intent:approve"),
        "prepare_sales_order_release_hold" => Some("sales_order_release_hold_intent:create"),
        "approve_sales_order_release_hold" => Some("sales_order_release_hold_intent:approve"),
        "get_business_master_record" => Some("business_master_data:read"),
        "get_business_product_master_record" => Some("business_product_master:read"),
        "prepare_core_master_creation" => Some("core_master_creation_intent:create"),
        "approve_core_master_creation" => Some("core_master_creation_intent:approve"),
        "prepare_core_master_status" => Some("core_master_status_intent:create"),
        "approve_core_master_status" => Some("core_master_status_intent:approve"),
        "prepare_product_master_status" => Some("product_master_status_intent:create"),
        "approve_product_master_status" => Some("product_master_status_intent:approve"),
        "prepare_core_master_update" => Some("core_master_update_intent:create"),
        "approve_core_master_update" => Some("core_master_update_intent:approve"),
        "prepare_product_master_creation" => Some("product_master_creation_intent:create"),
        "approve_product_master_creation" => Some("product_master_creation_intent:approve"),
        "prepare_product_master_update" => Some("product_master_update_intent:create"),
        "approve_product_master_update" => Some("product_master_update_intent:approve"),
        "prepare_crm_creation" => Some("crm_creation_intent:create"),
        "approve_crm_creation" => Some("crm_creation_intent:approve"),
        "prepare_crm_update" => Some("crm_update_intent:create"),
        "approve_crm_update" => Some("crm_update_intent:approve"),
        "prepare_crm_followup" => Some("crm_followup_intent:create"),
        "approve_crm_followup" => Some("crm_followup_intent:approve"),
        "prepare_inventory_count_creation" => Some("inventory_count_creation_intent:create"),
        "approve_inventory_count_creation" => Some("inventory_count_creation_intent:approve"),
        "prepare_inventory_count_submission" => Some("inventory_count_submission_intent:create"),
        "approve_inventory_count_submission" => Some("inventory_count_submission_intent:approve"),
        "prepare_inventory_count_posting" => Some("inventory_count_posting_intent:create"),
        "approve_inventory_count_posting" => Some("inventory_count_posting_intent:approve"),
        "prepare_inventory_count_cancellation" => {
            Some("inventory_count_cancellation_intent:create")
        }
        "approve_inventory_count_cancellation" => {
            Some("inventory_count_cancellation_intent:approve")
        }

        "prepare_sales_return_reversal" => Some("sales_return_reversal_intent:create"),
        "prepare_sales_return_cancellation" => Some("sales_return_cancellation_intent:create"),
        "approve_sales_return_reversal" => Some("sales_return_reversal_intent:approve"),
        "approve_sales_return_cancellation" => Some("sales_return_cancellation_intent:approve"),
        "prepare_purchase_return_reversal" => Some("purchase_return_reversal_intent:create"),
        "prepare_purchase_return_cancellation" => {
            Some("purchase_return_cancellation_intent:create")
        }
        "approve_purchase_return_reversal" => Some("purchase_return_reversal_intent:approve"),
        "approve_purchase_return_cancellation" => {
            Some("purchase_return_cancellation_intent:approve")
        }
        "update_sales_return_draft" => Some("sales_return:update_draft"),
        "update_purchase_return_draft" => Some("purchase_return:update_draft"),
        "create_sales_return_draft" => Some("sales_return:create"),
        "create_purchase_return_draft" => Some("purchase_return:create"),
        "prepare_sales_return_inspection" => Some("sales_return_inspection_intent:create"),
        "prepare_purchase_return_dispatch" => Some("purchase_return_dispatch_intent:create"),
        "prepare_purchase_return_acknowledgment" => {
            Some("purchase_return_acknowledgment_intent:create")
        }
        "approve_sales_return" => Some("sales_return:approve"),
        "approve_purchase_return" => Some("purchase_return:approve"),
        "approve_sales_return_inspection" => Some("sales_return_inspection_intent:approve"),
        "approve_purchase_return_dispatch" => Some("purchase_return_dispatch_intent:approve"),
        "approve_purchase_return_acknowledgment" => {
            Some("purchase_return_acknowledgment_intent:approve")
        }
        "prepare_shipment_reversal" => Some("shipment_reversal_intent:create"),
        "approve_shipment_reversal" => Some("shipment_reversal_intent:approve"),
        "prepare_goods_receipt_reversal" => Some("goods_receipt_reversal_intent:create"),
        "approve_goods_receipt_reversal" => Some("goods_receipt_reversal_intent:approve"),
        "prepare_inventory_opening_reversal" => Some("inventory_opening_reversal_intent:create"),
        "approve_inventory_opening_reversal" => Some("inventory_opening_reversal_intent:approve"),
        "prepare_sales_order_cancellation" => Some("sales_order_cancellation_intent:create"),
        "prepare_purchase_order_cancellation" => Some("purchase_order_cancellation_intent:create"),
        "approve_sales_order_cancellation" => Some("sales_order_cancellation_intent:approve"),
        "approve_purchase_order_cancellation" => Some("purchase_order_cancellation_intent:approve"),

        "get_customer_receipt_allocations" => Some("customer_receipt:read"),
        "get_supplier_payment_allocations" => Some("supplier_payment:read"),
        "prepare_customer_receipt_reversal" => Some("customer_receipt_reversal_intent:create"),
        "prepare_supplier_payment_reversal" => Some("supplier_payment_reversal_intent:create"),
        "prepare_receivable_allocation_reversal" => {
            Some("receivable_allocation_reversal_intent:create")
        }
        "prepare_payable_allocation_reversal" => Some("payable_allocation_reversal_intent:create"),
        "approve_customer_receipt_reversal" => Some("customer_receipt_reversal_intent:approve"),
        "approve_supplier_payment_reversal" => Some("supplier_payment_reversal_intent:approve"),
        "approve_receivable_allocation_reversal" => {
            Some("receivable_allocation_reversal_intent:approve")
        }
        "approve_payable_allocation_reversal" => Some("payable_allocation_reversal_intent:approve"),

        "search_sales_returns" => Some("sales_return:read"),
        "search_purchase_returns" => Some("purchase_return:read"),
        "get_sales_return_source" => Some("sales_return:read"),
        "get_purchase_return_source" => Some("purchase_return:read"),
        "get_sales_return_approval_preview" => Some("sales_return:read"),
        "get_purchase_return_approval_preview" => Some("purchase_return:read"),
        "search_shipments" => Some("shipment:read"),
        "search_goods_receipts" => Some("goods_receipt:read"),
        "search_inventory_openings"
        | "search_inventory_counts"
        | "get_inventory_count_approval_preview"
        | "get_inventory_count"
        | "search_inventory_count_options" => Some("inventory:read"),
        "search_customer_receipts" => Some("customer_receipt:read"),
        "search_supplier_payments" => Some("supplier_payment:read"),
        "search_receivables" => Some("receivable:read"),
        "search_payables" => Some("payable:read"),
        "search_crm_opportunities" | "get_crm_opportunity" => Some("crm:read"),
        "search_business_master_data" => Some("business_master_data:read"),
        "prepare_receivable_allocation" => Some("receivable_allocation_intent:create"),
        "approve_receivable_allocation" => Some("receivable_allocation_intent:approve"),
        "prepare_payable_allocation" => Some("payable_allocation_intent:create"),
        "approve_payable_allocation" => Some("payable_allocation_intent:approve"),

        "update_sales_order_draft" => Some("sales_order:update_draft"),
        "update_purchase_order_draft" => Some("purchase_order:update_draft"),
        "create_inventory_opening_draft" => Some("inventory_opening:create"),
        "create_sales_order_draft" => Some("sales_order:create"),
        "create_shipment_draft" => Some("shipment:create"),
        "create_purchase_order_draft" => Some("purchase_order:create"),
        "create_goods_receipt_draft" => Some("goods_receipt:create"),
        "create_customer_receipt_draft" => Some("customer_receipt:create"),
        "create_supplier_payment_draft" => Some("supplier_payment:create"),
        "approve_shipment" => Some("shipment:approve"),
        "approve_goods_receipt" => Some("goods_receipt:approve"),
        "approve_inventory_opening" => Some("inventory_opening:approve"),
        "approve_customer_receipt" => Some("customer_receipt:approve"),
        "get_customer_receipt_approval_preview" => Some("customer_receipt:read"),
        "approve_supplier_payment" => Some("supplier_payment:approve"),
        "get_supplier_payment_approval_preview" => Some("supplier_payment:read"),

        "get_shipment_approval_preview" => Some("shipment:read"),
        "get_goods_receipt_approval_preview" => Some("goods_receipt:read"),
        "get_inventory_opening_approval_preview" => Some("inventory:read"),
        "approve_sales_order" => Some("sales_order:approve"),
        "approve_purchase_order" => Some("purchase_order:approve"),
        "get_sales_order" | "search_sales_orders" | "get_sales_order_approval_preview" => {
            Some("sales_order:read")
        }
        "get_purchase_order" | "search_purchase_orders" | "get_purchase_order_approval_preview" => {
            Some("purchase_order:read")
        }
        "query_inventory_balance" => Some("inventory:read"),
        "query_receivables" => Some("receivable:read"),
        "query_payables" => Some("payable:read"),
        "query_order_profit"
        | "query_profitability_by_dimension"
        | "get_management_profit_report"
        | "get_management_report_snapshot"
        | "get_profit_evidence"
        | "get_operating_dashboard"
        | "get_business_data_quality" => Some("order_profit:read"),
        "search_business_anomalies"
        | "get_business_anomaly"
        | "analyze_order_profit_risks"
        | "analyze_receivable_risks"
        | "analyze_inventory_risks"
        | "analyze_purchase_cost_risks"
        | "analyze_cross_domain_risks"
        | "explain_profit_change" => Some("business_anomaly:read"),
        _ => None,
    }
}

pub(super) fn is_approval_tool(tool: &str) -> bool {
    ((super::adjustment_writes::family(tool).is_some()
        || super::inventory_count_writes::family(tool).is_some()
        || super::crm_writes::family(tool).is_some()
        || super::master_writes::family(tool).is_some()
        || super::order_hold_writes::family(tool).is_some()
        || super::operating_snapshot_writes::family(tool).is_some()
        || super::report_snapshot_writes::family(tool).is_some())
        && tool.starts_with("approve_"))
        || matches!(
            tool,
            "approve_sales_order"
                | "approve_sales_return"
                | "approve_purchase_return"
                | "approve_sales_return_inspection"
                | "approve_sales_return_reversal"
                | "approve_purchase_return_reversal"
                | "approve_sales_return_cancellation"
                | "approve_purchase_return_cancellation"
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
                | "approve_inventory_opening"
        )
}
