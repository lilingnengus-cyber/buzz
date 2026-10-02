use business_core::{
    b2::DomainError,
    service_delivery::{AcceptanceInput, DeliverableInput, Filters, ProjectInput, ServiceDelivery},
    PgStore,
};
use chrono::NaiveDate;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;
#[tokio::test]
async fn service_delivery_keeps_acceptance_auditable_and_scoped() {
    let Ok(url) = std::env::var("BUSINESS_CORE_SERVICE_TEST_DATABASE_URL") else {
        eprintln!("skipping: BUSINESS_CORE_SERVICE_TEST_DATABASE_URL unset");
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let service = ServiceDelivery::new(store);
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
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,'service_delivery:read'),($1,'service_delivery:manage'),($1,'business_product_master:read'),($1,'business_product_master:manage')").bind(role).execute(&pool).await.unwrap();
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
    let mut input = ProjectInput {
        title: "软件实施".into(),
        legal_entity_id: legal,
        business_unit_id: unit,
        customer_id: customer,
        owner_user_id: actor,
        contact_name: "张经理".into(),
        service_kind: "software_service".into(),
        sales_order_line_id: None,
        renewal_of_project_id: None,
        starts_on: NaiveDate::from_ymd_opt(2026, 9, 1),
        ends_on: NaiveDate::from_ymd_opt(2027, 9, 1),
        status: "pending".into(),
        description: "实施并开通软件".into(),
        expected_version: None,
    };
    let first = service
        .save_project(actor, Uuid::new_v4(), None, "service-create-001", &input)
        .await
        .unwrap();
    let id: Uuid = serde_json::from_value(first["id"].clone()).unwrap();
    assert_eq!(
        first,
        service
            .save_project(actor, Uuid::new_v4(), None, "service-create-001", &input)
            .await
            .unwrap()
    );
    assert!(matches!(
        service.detail(outsider, id).await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    assert!(service
        .list(outsider, &Filters::default(), false)
        .await
        .unwrap()["items"]
        .as_array()
        .unwrap()
        .is_empty());
    input.expected_version = Some(1);
    input.status = "active".into();
    service
        .save_project(actor, Uuid::new_v4(), Some(id), "service-start-001", &input)
        .await
        .unwrap();
    assert!(matches!(
        service
            .save_project(actor, Uuid::new_v4(), Some(id), "service-stale-001", &input)
            .await,
        Err(DomainError::VersionConflict)
    ));
    let mut task = DeliverableInput {
        title: "开通账号".into(),
        owner_user_id: actor,
        due_on: None,
        status: "active".into(),
        description: "".into(),
        evidence_url: "".into(),
        expected_version: None,
    };
    let item = service
        .save_deliverable(actor, Uuid::new_v4(), id, None, "service-task-001", &task)
        .await
        .unwrap();
    let task_id: Uuid = serde_json::from_value(item["id"].clone()).unwrap();
    input.expected_version = Some(3);
    input.status = "acceptance".into();
    service
        .save_project(
            actor,
            Uuid::new_v4(),
            Some(id),
            "service-submit-001",
            &input,
        )
        .await
        .unwrap();
    let mut accept = AcceptanceInput {
        accepted_on: NaiveDate::from_ymd_opt(2026, 10, 2).unwrap(),
        customer_reviewer: "张经理".into(),
        result: "passed".into(),
        note: "账号及功能核对".into(),
        evidence_url: "https://example.test/acceptance".into(),
        expected_version: 4,
    };
    assert!(matches!(
        service
            .accept(actor, Uuid::new_v4(), id, "service-accept-001", &accept)
            .await,
        Err(DomainError::Invalid(_))
    ));
    task.status = "completed".into();
    task.expected_version = Some(1);
    service
        .save_deliverable(
            actor,
            Uuid::new_v4(),
            id,
            Some(task_id),
            "service-task-done-001",
            &task,
        )
        .await
        .unwrap();
    accept.expected_version = 5;
    accept.result = "rejected".into();
    service
        .accept(actor, Uuid::new_v4(), id, "service-rejected-001", &accept)
        .await
        .unwrap();
    assert_eq!(
        service.detail(actor, id).await.unwrap()["item"]["status"],
        "active"
    );
    input.expected_version = Some(6);
    input.status = "acceptance".into();
    service
        .save_project(
            actor,
            Uuid::new_v4(),
            Some(id),
            "service-resubmit-001",
            &input,
        )
        .await
        .unwrap();
    accept.expected_version = 7;
    accept.result = "passed".into();
    let result = service
        .accept(actor, Uuid::new_v4(), id, "service-passed-001", &accept)
        .await
        .unwrap();
    assert_eq!(
        result,
        service
            .accept(actor, Uuid::new_v4(), id, "service-passed-001", &accept)
            .await
            .unwrap()
    );
    let detail = service.detail(actor, id).await.unwrap();
    assert_eq!(detail["item"]["status"], "completed");
    assert_eq!(detail["acceptances"].as_array().unwrap().len(), 2);
    assert!(matches!(
        service
            .save_deliverable(actor, Uuid::new_v4(), id, None, "closed-task-001", &task)
            .await,
        Err(DomainError::Invalid(_))
    ));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM trade_receivables")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let q = Filters {
        expiry: Some("expired".into()),
        today: NaiveDate::from_ymd_opt(2027, 9, 2),
        ..Filters::default()
    };
    assert_eq!(
        service.list(actor, &q, false).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let q = Filters { offset: 1, ..q };
    assert!(service.list(actor, &q, false).await.unwrap()["items"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(
        sqlx::query("DELETE FROM service_acceptances WHERE project_id=$1")
            .bind(id)
            .execute(&pool)
            .await
            .is_err()
    );
    let category = Uuid::new_v4();
    let uom = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO business_product_categories(id,code,name) VALUES($1,'SERVICE_CAT','服务')",
    )
    .bind(category)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_units_of_measure(id,code,name,precision_scale) VALUES($1,'PERSON_DAY','人天',2)").bind(uom).execute(&pool).await.unwrap();
    let products =
        business_core::product_master::ProductMasterService::new(PgStore::new(pool.clone()));
    let mut input:business_core::product_master::SaveProductMasterData=serde_json::from_value(serde_json::json!({"resourceType":"product","code":"TECH_IMPL","name":"实施服务","categoryId":category,"baseUomId":uom,"serviceKind":"technical_service"})).unwrap();
    let created = products
        .save(actor, Uuid::new_v4(), None, "service-product-001", &input)
        .await
        .unwrap();
    let products_list = products.list(actor, None, 100).await.unwrap();
    assert!(products_list
        .items
        .iter()
        .any(|p| p.id == created.id && p.service_kind.as_deref() == Some("technical_service")));
    input.expected_version = Some(created.version);
    input.service_kind = Some("goods".into());
    assert!(matches!(
        products
            .save(
                actor,
                Uuid::new_v4(),
                Some(created.id),
                "service-kind-change-001",
                &input
            )
            .await,
        Err(DomainError::Invalid(_))
    ));
}
