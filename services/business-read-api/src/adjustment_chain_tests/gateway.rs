use super::*;
use business_auth_gateway::Config;
use std::collections::HashSet;
pub(super) fn config(database_url: String, max_calls: i32) -> Config {
    Config {
        database_url,
        bind_addr: "127.0.0.1:0".parse().expect("addr"),
        authentik_issuer: "https://auth.test/application/o/workbench".into(),
        workbench_client_id: "workbench".into(),
        business_client_id: "business".into(),
        allowed_workbench_origins: HashSet::from(["tauri://localhost".into()]),
        business_origin: "https://business.test".into(),
        challenge_ttl: Duration::from_secs(90),
        embed_ttl: Duration::from_secs(30),
        business_ttl: Duration::from_secs(3600),
        rate_limit: 10,
        cleanup_interval: Duration::from_secs(60),
        cookie_name: "__Host-test".into(),
        cookie_secure: true,
        deployment_id: "test".into(),
        global_logout_redirect_uri: "https://workbench.test/".into(),
        business_agent_read_enabled: true,
        business_agent_draft_write_enabled: true,
        business_chat_approval_enabled: true,
        business_read_mcp_audience: "business-read-mcp".into(),
        agent_delegation_ttl: Duration::from_secs(300),
        agent_delegation_max_calls: max_calls,
        business_agent_rate_limit_per_minute: 100,
        business_read_service_credential: Some("test-service-credential-at-least-32-bytes".into()),
    }
}

pub(super) fn facts(trace_id: Uuid) -> RequestFacts {
    RequestFacts {
        ip: Some("127.0.0.1".into()),
        user_agent_hash: None,
        trace_id,
    }
}
