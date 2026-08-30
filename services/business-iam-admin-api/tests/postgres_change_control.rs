use business_iam_admin_api::{
    model::{Actor, CreateChangeRequest, Operation},
    Store,
};
use chrono::Utc;
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, Row};
use uuid::Uuid;

fn actor(principal_id: Uuid, subject: &str) -> Actor {
    Actor {
        principal_id,
        issuer: "https://auth.test/application/o/iam-admin/".into(),
        subject: subject.into(),
        auth_time: Utc::now(),
        evidence_hash: vec![7; 32],
    }
}

#[tokio::test]
async fn critical_change_allows_one_self_approval() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("TEST_DATABASE_URL absent; PostgreSQL change-control test skipped");
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("connect");
    Store::migrate(&pool).await.expect("migrate");
    sqlx::raw_sql(
        "TRUNCATE business_iam.admin_audit_events,business_iam.change_approvals,
         business_iam.change_requests,business_iam.authorization_decisions,
         business_iam.principal_permissions,business_iam.principal_roles,
         business_iam.role_permissions,business_iam.roles,business_iam.principals CASCADE",
    )
    .execute(&pool)
    .await
    .expect("truncate");

    let requester_id = Uuid::new_v4();
    let target_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO business_iam.principals(id,kind,external_id,display_name) VALUES
         ($1,'human','requester','Requester'),
         ($2,'independent_agent','finance-agent','Finance Agent')",
    )
    .bind(requester_id)
    .bind(target_id)
    .execute(&pool)
    .await
    .expect("principals");
    sqlx::query(
        "INSERT INTO business_iam.principal_permissions(principal_id,permission_id)
         SELECT $1,id FROM business_iam.permissions
          WHERE capability IN ('business_iam:read','business_iam:request','business_iam:approve')",
    )
    .bind(requester_id)
    .execute(&pool)
    .await
    .expect("admin grants");

    let runtime_pool = if let (Ok(runtime_url), Ok(runtime_role)) = (
        std::env::var("TEST_IAM_RUNTIME_DATABASE_URL"),
        std::env::var("TEST_IAM_RUNTIME_DATABASE_ROLE"),
    ) {
        Store::grant_runtime(&pool, &runtime_role)
            .await
            .expect("grant runtime");
        PgPoolOptions::new()
            .max_connections(5)
            .connect(&runtime_url)
            .await
            .expect("connect runtime")
    } else {
        pool.clone()
    };
    let store = Store::new(runtime_pool.clone());
    let requester = actor(requester_id, "requester-sub");
    let created = store
        .create_change(
            &requester,
            CreateChangeRequest {
                operation: Operation::PermissionGrant,
                payload: json!({
                    "externalId":"finance-agent",
                    "capability":"sales_order:write",
                    "dataScope":{"mode":"restricted","dimensions":{"legal_entity":["cn"]}},
                    "obligations":["human_approval"],
                    "expectedVersion":1
                }),
                reason: "Grant controlled sales-order maintenance".into(),
                idempotency_key: "grant-finance-agent-sales-write-v1".into(),
            },
            Uuid::new_v4(),
        )
        .await
        .expect("create");
    assert_eq!(created.risk_level, "critical");
    assert_eq!(created.required_approvals, 1);
    assert_eq!(created.status, "pending");

    let applied = store
        .approve(
            &requester,
            created.id,
            Some("Requester confirmed the scoped grant"),
            Uuid::new_v4(),
        )
        .await
        .expect("self approval");
    assert_eq!(applied.status, "applied");
    assert_eq!(applied.approval_count, 1);
    let grant = sqlx::query(
        "SELECT grant_row.data_scope,grant_row.obligations,principal.version
         FROM business_iam.principal_permissions grant_row
         JOIN business_iam.permissions permission ON permission.id=grant_row.permission_id
         JOIN business_iam.principals principal ON principal.id=grant_row.principal_id
         WHERE grant_row.principal_id=$1 AND permission.capability='sales_order:write'",
    )
    .bind(target_id)
    .fetch_one(&pool)
    .await
    .expect("applied grant");
    assert_eq!(
        grant.get::<serde_json::Value, _>("data_scope"),
        json!({"mode":"restricted","dimensions":{"legal_entity":["cn"]}})
    );
    assert_eq!(
        grant.get::<serde_json::Value, _>("obligations"),
        json!(["human_approval"])
    );
    assert_eq!(grant.get::<i64, _>("version"), 2);

    let audit_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM business_iam.admin_audit_events
         WHERE change_request_id=$1",
    )
    .bind(created.id)
    .fetch_one(&pool)
    .await
    .expect("audit count");
    assert_eq!(audit_count, 2);
    assert!(
        sqlx::query("DELETE FROM business_iam.change_approvals WHERE change_request_id=$1")
            .bind(created.id)
            .execute(&runtime_pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE business_iam.admin_audit_events SET result='failed'")
            .execute(&runtime_pool)
            .await
            .is_err()
    );
    assert!(sqlx::query(
        "UPDATE business_iam.permissions SET status='disabled' WHERE capability='inventory:read'"
    )
    .execute(&runtime_pool)
    .await
    .is_err());
}
