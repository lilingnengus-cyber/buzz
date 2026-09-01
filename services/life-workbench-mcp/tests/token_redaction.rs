use life_workbench_mcp::{client::ClientError, config::Config};
use std::collections::BTreeMap;
use uuid::Uuid;

#[test]
fn tokens_are_absent_from_debug_errors_and_safe_results() {
    let delegation = "ddddddddddddddddddddddddddddddddddddddddddd";
    assert_eq!(delegation.len(), 43);
    let service = "ssssssssssssssssssssssssssssssss";
    let trace_id = Uuid::new_v4();
    let config = Config::from_values(&BTreeMap::from([
        ("LIFE_DELEGATION_TOKEN".into(), delegation.into()),
        (
            "LIFE_AUTH_GATEWAY_URL".into(),
            "https://gateway.test".into(),
        ),
        ("LIFE_API_URL".into(), "https://life.test".into()),
        ("LIFE_WORKBENCH_MCP_SERVICE_TOKEN".into(), service.into()),
        ("LIFE_AGENT_ID".into(), "life-agent".into()),
        ("LIFE_AGENT_TURN_ID".into(), "turn-1".into()),
        ("LIFE_TRACE_ID".into(), trace_id.to_string()),
    ]))
    .expect("config");
    let debug = format!("{config:?}");
    assert!(!debug.contains(delegation));
    assert!(!debug.contains(service));

    for error in [
        ClientError::Validation,
        ClientError::ScopeDenied,
        ClientError::RateLimited,
        ClientError::GatewayUnavailable,
        ClientError::LifeApiUnavailable,
        ClientError::InvalidResponse,
        ClientError::Internal,
    ] {
        let debug = format!("{error:?}");
        let display = error.to_string();
        let result = error.safe_result(trace_id);
        for secret in [delegation, service, "grant-secret"] {
            assert!(!debug.contains(secret));
            assert!(!display.contains(secret));
            assert!(!result.contains(secret));
        }
        assert!(!result.contains("Prisma"));
        assert!(!result.contains("SELECT"));
    }
}
