use business_core::{
    b2::DomainError,
    crm::{AddFollowup, CrmService, Filters, SaveOpportunity},
    PgStore,
};
use chrono::NaiveDate;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;
#[path = "support/crm_agent_reads.rs"]
mod crm_agent_reads;
#[path = "support/crm_intents.rs"]
mod crm_intents;
#[path = "support/crm_write_authority.rs"]
mod crm_write_authority;
#[tokio::test]
async fn crm_persists_scoped_followups_and_rejects_conflicts() {
    let Ok(url) = std::env::var("BUSINESS_CORE_CRM_TEST_DATABASE_URL") else {
        eprintln!("skipping: BUSINESS_CORE_CRM_TEST_DATABASE_URL unset");
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let crm = CrmService::new(store);
    let actor = Uuid::new_v4();
    let outsider = Uuid::new_v4();
    let legal = Uuid::new_v4();
    let unit = Uuid::new_v4();
    let customer_unit = Uuid::new_v4();
    let compatibility_legal = Uuid::new_v4();
    let customer = Uuid::new_v4();
    let role = Uuid::new_v4();
    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'crm-test',$1::text,'CRM User'),($2,'crm-test',$2::text,'Other User')").bind(actor).bind(outsider).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_roles(id,role_key,name) VALUES($1,'crm_test','CRM Test')")
        .bind(role)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,'crm:read'),($1,'crm:manage')").bind(role).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) VALUES($1,$3,$1),($2,$3,$1)").bind(actor).bind(outsider).bind(role).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_legal_entities(id,code,name,country_code,functional_currency) VALUES($1,'CRM_LE','CRM LE','CN','CNY')").bind(legal).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_legal_entities(id,code,name,country_code,functional_currency) VALUES($1,'CRM_LE_COMPAT','CRM Compatibility LE','CN','CNY')").bind(compatibility_legal).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_units(id,legal_entity_id,code,name) VALUES($1,$2,'CRM_BU','CRM BU')",
    )
    .bind(unit)
    .bind(compatibility_legal)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_units(id,legal_entity_id,code,name) VALUES($1,$2,'CRM_CUSTOMER_BU','CRM Customer BU')").bind(customer_unit).bind(legal).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_customers(id,legal_entity_id,business_unit_id,code,name,credit_currency) VALUES($1,$2,$3,'CRM_C','CRM Customer','CNY')").bind(customer).bind(legal).bind(customer_unit).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_legal_entity_scopes(enterprise_user_id,legal_entity_id,granted_by) VALUES($1,$2,$1)").bind(actor).bind(legal).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)").bind(actor).bind(unit).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)").bind(actor).bind(customer).execute(&pool).await.unwrap();
    let mut input = SaveOpportunity {
        legal_entity_id: legal,
        business_unit_id: unit,
        customer_id: Some(customer),
        title: "企业采购".into(),
        company_name: "测试公司".into(),
        contact_name: "张经理".into(),
        contact_details: "".into(),
        stage: "new".into(),
        expected_amount_minor: Some(125050),
        currency: "CNY".into(),
        next_action: "发送方案".into(),
        next_follow_up: NaiveDate::from_ymd_opt(2026, 9, 19),
        expected_version: None,
    };
    let first = crm
        .save(actor, Uuid::new_v4(), None, "create-key-0001", &input)
        .await
        .unwrap();
    let id: Uuid = serde_json::from_value(first["id"].clone()).unwrap();
    assert_eq!(
        crm.save(actor, Uuid::new_v4(), None, "create-key-0001", &input)
            .await
            .unwrap(),
        first
    );
    let details = crm.detail(actor, id, 0).await.unwrap();
    assert_eq!(details["item"]["companyName"], "测试公司");
    assert!(matches!(
        crm.detail(outsider, id, 0).await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    assert!(
        crm.list(outsider, &Filters::default()).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(crm.options(outsider).await.unwrap()["items"]
        .as_array()
        .unwrap()
        .is_empty());
    let note = AddFollowup {
        note: "已沟通需求，准备报价".into(),
        stage: "quoting".into(),
        next_action: "确认报价".into(),
        next_follow_up: input.next_follow_up,
        expected_version: 1,
    };
    let result = crm
        .followup(actor, Uuid::new_v4(), id, "followup-key-1", &note)
        .await
        .unwrap();
    assert_eq!(result["version"], 2);
    assert_eq!(
        crm.followup(actor, Uuid::new_v4(), id, "followup-key-1", &note)
            .await
            .unwrap(),
        result
    );
    assert!(matches!(
        crm.followup(actor, Uuid::new_v4(), id, "followup-key-2", &note)
            .await,
        Err(DomainError::VersionConflict)
    ));
    let detail = crm.detail(actor, id, 0).await.unwrap();
    assert_eq!(detail["followups"].as_array().unwrap().len(), 1);
    assert_eq!(detail["item"]["stage"], "quoting");
    for contacts in [false, true] {
        let page = crm
            .register(actor, &Filters::default(), contacts)
            .await
            .unwrap();
        assert_eq!(page["items"].as_array().unwrap().len(), 1);
        let hidden = crm
            .register(outsider, &Filters::default(), contacts)
            .await
            .unwrap();
        assert!(hidden["items"].as_array().unwrap().is_empty());
        let missing = Filters {
            query: Some("不存在%".into()),
            ..Default::default()
        };
        assert!(
            crm.register(actor, &missing, contacts).await.unwrap()["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let next = Filters {
            offset: 50,
            ..Default::default()
        };
        assert!(crm.register(actor, &next, contacts).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty());
    }
    let contacts = crm
        .register(actor, &Filters::default(), true)
        .await
        .unwrap();
    assert_eq!(contacts["items"][0]["contactName"], "张经理");
    assert_eq!(
        contacts["items"][0]["opportunities"][0]["id"],
        id.to_string()
    );
    let history = crm
        .register(actor, &Filters::default(), false)
        .await
        .unwrap();
    assert_eq!(history["items"][0]["note"], note.note);
    input.expected_version = Some(2);
    input.stage = "won".into();
    crm.save(actor, Uuid::new_v4(), Some(id), "update-key-1", &input)
        .await
        .unwrap();
    let due = Filters {
        due_by: NaiveDate::from_ymd_opt(2026, 9, 20),
        ..Default::default()
    };
    assert!(crm.list(actor, &due).await.unwrap()["items"]
        .as_array()
        .unwrap()
        .is_empty());
    input.title = "changed".into();
    assert!(matches!(
        crm.save(actor, Uuid::new_v4(), Some(id), "update-key-1", &input)
            .await,
        Err(DomainError::IdempotencyConflict)
    ));
    input.expected_version = None;
    input.stage = "invented".into();
    assert!(matches!(
        crm.save(actor, Uuid::new_v4(), None, "invalid-key-1", &input)
            .await,
        Err(DomainError::Invalid(_))
    ));
    sqlx::query("DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1")
        .bind(actor)
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        crm.detail(actor, id, 0).await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    assert!(matches!(
        crm.followup(actor, Uuid::new_v4(), id, "revoked-key-1", &note)
            .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    for contacts in [false, true] {
        assert!(crm
            .register(actor, &Filters::default(), contacts)
            .await
            .unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty());
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sales_orders")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM business_core_audit_events WHERE target_type='crm_opportunity'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit, 3);
    crm_write_authority::check(&pool, actor, customer, id).await;
    crm_intents::check(&pool, actor, customer, legal, unit).await;
    crm_agent_reads::check(&pool, actor, outsider).await;
    // Every CRM route stays behind the existing browser-session middleware.
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    if let Ok(config) = business_core::Config::from_env() {
        let state = business_core::AppState::new(PgStore::new(pool.clone()), &config);
        let router = business_core::router(state);
        for (method, path) in [
            ("GET", "/api/v1/crm/options".to_string()),
            ("GET", "/api/v1/crm/opportunities".into()),
            ("GET", "/api/v1/crm/followups".into()),
            ("GET", "/api/v1/crm/contacts".into()),
            ("POST", "/api/v1/crm/opportunities".into()),
            ("PUT", format!("/api/v1/crm/opportunities/{id}")),
            ("POST", format!("/api/v1/crm/opportunities/{id}/followups")),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
        }
    }
}
