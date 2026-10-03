use uuid::Uuid;
pub(super) const AGENT_SCOPES: [&str; 18] = [
    "crm:read",
    "crm:manage",
    "business_master_data:read",
    "business_master_data:manage",
    "sales_order:read",
    "purchase_order:read",
    "inventory:read",
    "receivable:read",
    "payable:read",
    "order_profit:read",
    "business_anomaly:read",
    "business_action:read",
    "sales_order:create",
    "shipment:create",
    "purchase_order:create",
    "goods_receipt:create",
    "customer_receipt:create",
    "supplier_payment:create",
];

pub(super) fn chat_approval_scope(content: &str) -> Option<&'static str> {
    let mut parts = content.split_whitespace();
    if !matches!(parts.next()?, "/approve" | "/reject") {
        return None;
    }
    let scope = match parts.next()? {
        "sales-order" => "sales_order:approve",
        "purchase-order" => "purchase_order:approve",
        _ => return None,
    };
    let _: Uuid = parts.next()?.parse().ok()?;
    let version = parts.next()?.strip_prefix('v')?.parse::<i64>().ok()?;
    let hash = parts.next()?;
    if parts.next().is_some()
        || version <= 0
        || hash.len() != 64
        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some(scope)
}
