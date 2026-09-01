use axum::http::{header, HeaderMap, HeaderValue};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use life_auth_gateway::{
    agent::{AgentError, ConsumeDelegationRequest, DelegationPolicy, ResourceContext},
    auth::bearer,
    call_grant::CallGrantSigner,
    security::{ServiceToken, SigningKeyMaterial},
    Store,
};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    AssertSqlSafe, PgPool, Row,
};
use std::{str::FromStr, time::Duration};
use uuid::Uuid;

struct TestDatabase {
    admin: PgPool,
    pool: PgPool,
    schema: String,
}

impl TestDatabase {
    async fn create() -> Self {
        let url = std::env::var("LIFE_AUTH_TEST_DATABASE_URL")
            .expect("LIFE_AUTH_TEST_DATABASE_URL must name an isolated PostgreSQL database");
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect test database");
        let schema = format!("life_domain_test_{}", Uuid::new_v4().simple());
        sqlx::raw_sql(AssertSqlSafe(format!("CREATE SCHEMA \"{schema}\"")))
            .execute(&admin)
            .await
            .expect("create isolated schema");
        let options = PgConnectOptions::from_str(&url)
            .expect("parse database URL")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .expect("connect isolated schema");
        Store::migrate(&pool).await.expect("run Life migrations");
        Self {
            admin,
            pool,
            schema,
        }
    }

    async fn cleanup(self) {
        self.pool.close().await;
        sqlx::raw_sql(AssertSqlSafe(format!(
            "DROP SCHEMA \"{}\" CASCADE",
            self.schema
        )))
        .execute(&self.admin)
        .await
        .expect("drop isolated schema");
        self.admin.close().await;
    }
}

fn consume_request() -> ConsumeDelegationRequest {
    ConsumeDelegationRequest {
        agent_id: "life-agent".into(),
        agent_turn_id: "turn-domain-isolation".into(),
        tool: "get_action_detail".into(),
        capability: "action:read".into(),
        resource: ResourceContext {
            resource_type: "action".into(),
            id: "action-1".into(),
            expected_version: None,
        },
        normalized_input_hash: format!("sha256:{}", "1".repeat(64)),
        idempotency_key: Uuid::new_v4().to_string(),
        trace_id: Uuid::new_v4(),
    }
}

#[test]
fn browser_cookies_are_not_life_authorization_credentials() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        HeaderValue::from_static(
            "__Host-bizfin_business=business-session; __Host-hermes=hermes-session",
        ),
    );
    assert!(bearer(&headers).is_none());

    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Cookie __Host-bizfin_business=business-session"),
    );
    assert!(bearer(&headers).is_none());
}

#[test]
fn life_credentials_and_audiences_cannot_cross_into_other_domains() {
    let life_service = "l".repeat(32);
    let business_service =
        ServiceToken::parse("BUSINESS", "b".repeat(32)).expect("business service credential");
    let hermes_service =
        ServiceToken::parse("HERMES", "h".repeat(32)).expect("Hermes service credential");
    assert!(!business_service.matches(&life_service));
    assert!(!hermes_service.matches(&life_service));

    for foreign_audience in ["business-read-mcp", "hermes"] {
        assert!(
            DelegationPolicy::new(foreign_audience, "life-test", Duration::from_secs(300)).is_err()
        );
    }

    let key = SigningKeyMaterial::parse(&"11".repeat(32)).expect("signing key");
    for foreign_audience in ["business-read-api", "business-action-service", "hermes"] {
        assert!(CallGrantSigner::new(
            "life-auth-gateway",
            foreign_audience,
            Duration::from_secs(30),
            key.clone(),
        )
        .is_err());
    }
}

#[tokio::test]
async fn foreign_tokens_fail_closed_and_life_schema_has_no_cross_domain_dependency() {
    let database = TestDatabase::create().await;
    let store = Store::new(database.pool.clone());
    let signer = CallGrantSigner::new(
        "life-auth-gateway",
        "lifeos-workbench-api",
        Duration::from_secs(30),
        SigningKeyMaterial::parse(&"22".repeat(32)).expect("signing key"),
    )
    .expect("call grant signer");

    for (domain, token) in [
        ("Business", URL_SAFE_NO_PAD.encode([0xb1_u8; 32])),
        ("Hermes", URL_SAFE_NO_PAD.encode([0xe5_u8; 32])),
    ] {
        let result = store
            .consume_agent_delegation(&token, consume_request(), &signer)
            .await;
        assert!(
            matches!(result, Err(AgentError::Unauthorized)),
            "{domain} delegation credential must be rejected by the Life consume boundary"
        );
    }

    let tables = sqlx::query_scalar::<_, String>(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema=current_schema() AND table_type='BASE TABLE'",
    )
    .fetch_all(&database.pool)
    .await
    .expect("list Life tables");
    assert!(tables.iter().any(|table| table == "life_agent_delegations"));
    assert!(tables
        .iter()
        .all(|table| table == "_sqlx_migrations" || table.starts_with("life_")));

    let foreign_keys = sqlx::query(
        "SELECT target_namespace.nspname AS target_schema,
                target_table.relname AS target_table
         FROM pg_constraint constraint_row
         JOIN pg_class source_table ON source_table.oid=constraint_row.conrelid
         JOIN pg_namespace source_namespace ON source_namespace.oid=source_table.relnamespace
         JOIN pg_class target_table ON target_table.oid=constraint_row.confrelid
         JOIN pg_namespace target_namespace ON target_namespace.oid=target_table.relnamespace
         WHERE constraint_row.contype='f' AND source_namespace.nspname=current_schema()",
    )
    .fetch_all(&database.pool)
    .await
    .expect("inspect Life foreign keys");
    assert!(!foreign_keys.is_empty());
    for foreign_key in foreign_keys {
        assert_eq!(
            foreign_key.get::<String, _>("target_schema"),
            database.schema
        );
        assert!(foreign_key
            .get::<String, _>("target_table")
            .starts_with("life_"));
    }

    database.cleanup().await;
}
