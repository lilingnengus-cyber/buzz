use life_auth_gateway::audit::{AuditCorrelation, AuditOutcome};
use uuid::Uuid;

#[test]
fn audit_contract_has_only_fixed_codes_and_trace_identifier() {
    let edge = AuditCorrelation {
        trace_id: Uuid::nil(),
        event_type: "LIFE_DISCLOSURE_ALLOWED",
        outcome: AuditOutcome::Success,
        reason_code: Some("policy_current"),
    };
    let debug = format!("{edge:?}");
    for forbidden in [
        "token",
        "cookie",
        "prompt",
        "journal",
        "private_key",
        "authorization",
    ] {
        assert!(!debug.to_ascii_lowercase().contains(forbidden));
    }
}

#[test]
fn metric_source_does_not_admit_identity_labels() {
    let source = include_str!("../src/metrics.rs");
    for forbidden in ["user", "resource", "workspace", "pubkey", "trace"] {
        assert!(!source.contains(&format!("\"{forbidden}\" =>")));
    }
}
