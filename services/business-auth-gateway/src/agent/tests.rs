use super::*;
use nostr::{EventBuilder, Keys, Kind};

#[test]
fn settlement_commands_bind_exact_record_family() {
    for kind in [
        "operating-report-snapshot-intent",
        "management-report-snapshot-intent",
        "sales-order-hold-intent",
        "sales-order-release-hold-intent",
        "core-master-status-intent",
        "product-master-status-intent",
        "core-master-creation-intent",
        "core-master-update-intent",
        "product-master-creation-intent",
        "product-master-update-intent",
        "crm-creation-intent",
        "crm-update-intent",
        "crm-followup-intent",
        "inventory-count-creation-intent",
        "inventory-count-submission-intent",
        "inventory-count-posting-intent",
        "inventory-count-cancellation-intent",
        "sales-return-reversal-intent",
        "purchase-return-reversal-intent",
        "sales-return-cancellation-intent",
        "purchase-return-cancellation-intent",
        "sales-return",
        "purchase-return",
        "sales-return-inspection-intent",
        "purchase-return-dispatch-intent",
        "purchase-return-acknowledgment-intent",
        "shipment-reversal-intent",
        "goods-receipt-reversal-intent",
        "inventory-opening-reversal-intent",
        "sales-order-cancellation-intent",
        "purchase-order-cancellation-intent",
        "customer-receipt-reversal-intent",
        "supplier-payment-reversal-intent",
        "receivable-allocation-reversal-intent",
        "payable-allocation-reversal-intent",
        "customer-receipt",
        "supplier-payment",
        "receivable-allocation-intent",
        "payable-allocation-intent",
    ] {
        let command = format!("确认 {kind} {} v1 {}", Uuid::new_v4(), "a".repeat(64));
        let parsed = parse_chat_approval_command(&command).expect("valid signed command syntax");
        assert_eq!(parsed.document_type, kind.replace('-', "_"));
        assert!(!scope_is_allowed(parsed.required_scope, true, false));
        assert!(scope_is_allowed(parsed.required_scope, true, true));
    }
}

#[test]
fn source_event_must_carry_matching_channel() {
    let keys = Keys::generate();
    let event = EventBuilder::new(Kind::TextNote, "query")
        .tags([nostr::Tag::custom(
            nostr::TagKind::Custom("h".into()),
            ["channel-a"],
        )])
        .sign_with_keys(&keys)
        .expect("sign");
    assert!(source_event_has_channel(&event, "channel-a"));
    assert!(!source_event_has_channel(&event, "channel-b"));
}

#[test]
fn only_fixed_agent_scopes_are_accepted() {
    assert!(AGENT_SCOPES.contains(&"inventory:read"));
    assert!(AGENT_SCOPES.contains(&"sales_order:create"));
    assert!(!AGENT_SCOPES.contains(&"sales_order:confirm"));
    assert!(!AGENT_SCOPES.contains(&"payment:execute"));
    assert!(scope_is_allowed("sales_order:read", false, false));
    assert!(!scope_is_allowed("sales_order:create", false, false));
    assert!(scope_is_allowed("sales_order:create", true, false));
    assert!(!scope_is_allowed("sales_order:approve", true, false));
    assert!(scope_is_allowed("sales_order:approve", false, true));
}

#[test]
fn approval_command_is_exact_and_binds_every_authority_field() {
    let id = Uuid::new_v4();
    let hash = "b".repeat(64);
    let command = parse_chat_approval_command(&format!("/approve sales-order {id} v7 {hash}"))
        .expect("valid command");
    assert_eq!(command.decision, "approve");
    assert_eq!(command.document_type, "sales_order");
    assert_eq!(command.document_id, id);
    assert_eq!(command.expected_version, 7);
    assert_eq!(command.preview_hash, hash);
    assert_eq!(command.required_scope, "sales_order:approve");
    assert!(parse_chat_approval_command("同意").is_none());
    assert!(parse_chat_approval_command(&format!(
        "/approve sales-order {id} v7 {} extra",
        "b".repeat(64)
    ))
    .is_none());
}
