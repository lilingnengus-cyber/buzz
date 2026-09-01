use life_auth_gateway::{
    catalog,
    iam::{AuthorizationRequest, ObligationSatisfaction},
    identity::SessionPrincipal,
    membership::{MembershipEvent, MembershipSnapshot},
    model::{IdentityBindingId, LifeWorkbenchUserId, WorkbenchSessionId},
    Store,
};
use life_iam::{
    Capability, CapabilityGrant, CapabilityRequest, ConversationContext, DataScope, Obligation,
    ScopeSet,
};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    AssertSqlSafe, PgPool,
};
use std::{collections::BTreeMap, str::FromStr};
use uuid::Uuid;

struct TestDatabase {
    admin: PgPool,
    pool: PgPool,
    schema: String,
}

impl TestDatabase {
    async fn create() -> Option<Self> {
        let url = std::env::var("LIFE_AUTH_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect test database");
        let schema = format!("life_auth_test_{}", Uuid::new_v4().simple());
        sqlx::raw_sql(AssertSqlSafe(format!("CREATE SCHEMA \"{schema}\"")))
            .execute(&admin)
            .await
            .expect("create isolated schema");
        let options = PgConnectOptions::from_str(&url)
            .expect("parse test database URL")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await
            .expect("connect isolated schema");
        Store::migrate(&pool).await.expect("run Life migrations");
        Some(Self {
            admin,
            pool,
            schema,
        })
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

fn capability(value: &str) -> Capability {
    Capability::parse(value).expect("test capability")
}

fn workspace_scope() -> DataScope {
    DataScope {
        workspaces: ScopeSet::restricted(["workspace-1"]).expect("workspace scope"),
        ..DataScope::default()
    }
}

fn request(
    principal: &SessionPrincipal,
    binding_id: IdentityBindingId,
    agent_id: &str,
    capabilities: &[&str],
) -> AuthorizationRequest {
    let requested = capabilities
        .iter()
        .map(|name| {
            (
                capability(name),
                CapabilityRequest {
                    data_scope: workspace_scope(),
                    obligations: Default::default(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let runtime_ceiling = capabilities
        .iter()
        .map(|name| {
            (
                capability(name),
                CapabilityGrant {
                    data_scope: workspace_scope(),
                    obligations: Default::default(),
                },
            )
        })
        .collect();
    AuthorizationRequest {
        principal: principal.clone(),
        identity_binding_id: binding_id,
        agent_id: agent_id.into(),
        agent_turn_id: Uuid::new_v4().to_string(),
        source_event_id: Some("c".repeat(64)),
        requested,
        runtime_ceiling,
        conversation: ConversationContext::DirectMessage,
        satisfaction: ObligationSatisfaction::default(),
        batch_size: 1,
        disclosure_allowed: false,
        trace_id: Uuid::new_v4(),
    }
}

async fn seed_human(pool: &PgPool) -> (SessionPrincipal, IdentityBindingId) {
    let user_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO life_workbench_users
         (id,oidc_issuer,oidc_subject,life_os_user_id,status)
         VALUES($1,'https://identity.example','subject','life-user','active')",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .expect("seed user");
    sqlx::query(
        "INSERT INTO life_workbench_sessions
         (id,workbench_user_id,deployment_id,token_hash,status,expires_at)
         VALUES($1,$2,'life-test',$3,'active',now()+interval '1 hour')",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(vec![51_u8; 32])
    .execute(pool)
    .await
    .expect("seed session");
    let binding_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO life_identity_bindings
         (id,workbench_user_id,buzz_pubkey,source_event_id,status)
         VALUES($1,$2,$3,$4,'active')",
    )
    .bind(binding_id)
    .bind(user_id)
    .bind("b".repeat(64))
    .bind("d".repeat(64))
    .execute(pool)
    .await
    .expect("seed identity binding");
    (
        SessionPrincipal {
            session_id: WorkbenchSessionId::new(session_id),
            user_id: LifeWorkbenchUserId::new(user_id),
            issuer: "https://identity.example".into(),
            subject: "subject".into(),
            life_os_user_id: "life-user".into(),
            deployment_id: "life-test".into(),
        },
        IdentityBindingId::new(binding_id),
    )
}

async fn membership(store: &Store, version: i64, role: &str) {
    store
        .apply_membership_event(&MembershipEvent {
            life_os_user_id: "life-user".into(),
            user_active: true,
            membership_version: version,
            memberships: vec![MembershipSnapshot {
                workspace_id: "workspace-1".into(),
                role: role.into(),
            }],
            trace_id: Uuid::new_v4(),
        })
        .await
        .expect("apply membership");
}

#[tokio::test]
async fn catalog_and_authorization_are_complete_current_and_fail_closed() {
    let expected = [
        "workspace:read",
        "domain:read",
        "domain:create",
        "domain:update",
        "goal:read",
        "goal:create",
        "goal:update",
        "goal:archive",
        "project:read",
        "project:create",
        "project:update",
        "project:archive",
        "action:read",
        "action:create",
        "action:update",
        "action:status_update",
        "action:reorder",
        "action:delete",
        "focus:read",
        "focus:update",
        "focus:replace",
        "calendar:read",
        "calendar:create",
        "calendar:update",
        "calendar:delete",
        "calendar:invite",
        "journal:read",
        "journal:create",
        "journal:update",
        "journal:delete",
        "knowledge:read",
        "knowledge:create",
        "knowledge:update",
        "knowledge:delete",
        "knowledge:export",
        "review:read",
        "review:create",
        "review:update",
        "ai_execution:read",
        "ai_execution:start",
        "ai_execution:append_output",
        "ai_execution:finish",
        "ai_execution:policy_update",
        "write_command:preview",
        "write_command:execute",
        "notification:read",
        "notification:acknowledge",
    ];
    assert_eq!(catalog::entries().len(), expected.len());
    for capability in expected {
        assert!(
            catalog::capability(capability).is_some(),
            "missing {capability}"
        );
    }
    assert_eq!(
        catalog::tool("update_action_status")
            .expect("known tool")
            .capability,
        "action:status_update"
    );
    assert!(catalog::tool("run_sql").is_none());

    let Some(database) = TestDatabase::create().await else {
        eprintln!("LIFE_AUTH_TEST_DATABASE_URL absent; IAM integration test skipped");
        return;
    };
    catalog::validate_persisted(&database.pool)
        .await
        .expect("validate seeded catalog");
    let store = Store::new(database.pool.clone());
    let (principal, binding_id) = seed_human(&database.pool).await;
    membership(&store, 1, "OWNER").await;

    let allowed = store
        .authorize(request(
            &principal,
            binding_id,
            "proxy-agent",
            &["action:update"],
        ))
        .await
        .expect("authorize owner");
    assert!(allowed.decision.allowed);

    membership(&store, 2, "VIEWER").await;
    let mut disclosure = request(&principal, binding_id, "proxy-agent", &["action:update"]);
    disclosure.disclosure_allowed = true;
    let denied = store.authorize(disclosure).await.expect("authorize viewer");
    assert!(
        !denied.decision.allowed,
        "channel disclosure must never grant write"
    );

    let agent_principal = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO life_principals(id,agent_id,kind,status)
         VALUES($1,'independent-active','independent_agent','active')",
    )
    .bind(agent_principal)
    .execute(&database.pool)
    .await
    .expect("seed independent principal");
    sqlx::query(
        "INSERT INTO life_principal_capabilities
         (principal_id,capability,catalog_version,data_scope,obligations,status)
         VALUES($1,'action:read',1,$2,'[]','active')",
    )
    .bind(agent_principal)
    .bind(serde_json::to_value(workspace_scope()).expect("serialize scope"))
    .execute(&database.pool)
    .await
    .expect("seed independent authority");
    let independent = store
        .authorize(request(
            &principal,
            binding_id,
            "independent-active",
            &["action:read", "action:update"],
        ))
        .await
        .expect("authorize independent agent");
    assert!(independent
        .decision
        .allowed_capabilities
        .contains(&capability("action:read")));
    assert!(independent
        .decision
        .denied_capabilities
        .contains(&capability("action:update")));

    sqlx::query(
        "INSERT INTO life_principals(id,agent_id,kind,status)
         VALUES($1,'independent-disabled','independent_agent','disabled')",
    )
    .bind(Uuid::new_v4())
    .execute(&database.pool)
    .await
    .expect("seed disabled independent principal");
    let disabled = store
        .authorize(request(
            &principal,
            binding_id,
            "independent-disabled",
            &["action:read"],
        ))
        .await
        .expect("authorize disabled independent agent");
    assert!(
        !disabled.decision.allowed,
        "disabled independent Agent cannot fall back to human"
    );

    membership(&store, 3, "OWNER").await;
    let unsatisfied = store
        .authorize(request(
            &principal,
            binding_id,
            "proxy-agent",
            &["action:delete"],
        ))
        .await
        .expect("evaluate unsatisfied obligation");
    assert!(!unsatisfied.decision.allowed);
    let mut confirmed = request(&principal, binding_id, "proxy-agent", &["action:delete"]);
    confirmed.satisfaction.human_confirmation = true;
    confirmed.satisfaction.step_up_authentication = true;
    let confirmed = store
        .authorize(confirmed)
        .await
        .expect("authorize confirmed delete");
    assert!(confirmed.decision.allowed);
    assert!(confirmed
        .decision
        .grants
        .get(&capability("action:delete"))
        .expect("delete grant")
        .obligations
        .contains(&Obligation::HumanConfirmation));

    let mut unknown = request(&principal, binding_id, "proxy-agent", &["action:read"]);
    unknown.requested.insert(
        capability("custom:invented"),
        CapabilityRequest {
            data_scope: workspace_scope(),
            obligations: Default::default(),
        },
    );
    let unknown = store
        .authorize(unknown)
        .await
        .expect("evaluate unknown capability");
    assert!(
        !unknown.decision.allowed,
        "unknown capability must fail the whole request"
    );

    store
        .mark_membership_sync_failed(principal.user_id, Uuid::new_v4())
        .await
        .expect("mark sync failed");
    assert!(store
        .authorize(request(
            &principal,
            binding_id,
            "proxy-agent",
            &["action:read"]
        ))
        .await
        .is_err());
    assert!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM life_iam_decisions")
            .fetch_one(&database.pool)
            .await
            .expect("decision count")
            >= 7
    );
    let decision_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM life_iam_decisions ORDER BY created_at LIMIT 1",
    )
    .fetch_one(&database.pool)
    .await
    .expect("decision id");
    assert!(
        sqlx::query("UPDATE life_iam_decisions SET decision_reason='tampered' WHERE id=$1")
            .bind(decision_id)
            .execute(&database.pool)
            .await
            .is_err()
    );
    assert!(sqlx::query("DELETE FROM life_iam_decisions WHERE id=$1")
        .bind(decision_id)
        .execute(&database.pool)
        .await
        .is_err());

    sqlx::query(
        "INSERT INTO life_capability_catalog
         (capability,allowed_tools,risk_class,requires_expected_version,
          default_max_calls,max_batch_size,obligations,catalog_version,status)
         VALUES('action:update','[]','low',true,25,25,'[]',2,'retired')",
    )
    .execute(&database.pool)
    .await
    .expect("seed invalid risk downgrade");
    assert!(catalog::validate_persisted(&database.pool).await.is_err());

    database.cleanup().await;
}
