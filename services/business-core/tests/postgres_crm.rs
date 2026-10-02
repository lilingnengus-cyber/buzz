use business_core::{
    b2::DomainError,
    crm::{AddFollowup, CrmService, Filters, SaveAccount, SaveContact, SaveOpportunity},
    PgStore,
};
use chrono::NaiveDate;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;
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
        account_id: None,
        contact_id: None,
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
        owner_user_id: None,
        expected_close_date: None,
        loss_reason: None,
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
    assert_eq!(details["item"]["companyName"], "CRM Customer");
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
    let options = crm.options(actor).await.unwrap();
    let operating_unit = options["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["resourceType"] == "business_unit" && item["id"] == unit.to_string())
        .unwrap();
    assert!(operating_unit["legalEntityId"].is_null());
    assert!(operating_unit["parentBusinessUnitId"].is_null());
    assert_eq!(operating_unit["status"], "active");
    assert_eq!(
        operating_unit["ancestorPath"],
        serde_json::json!(["CRM BU"])
    );
    let note = AddFollowup {
        note: "已沟通需求，准备报价".into(),
        stage: "quoting".into(),
        next_action: "确认报价".into(),
        next_follow_up: input.next_follow_up,
        expected_version: 1,
        loss_reason: None,
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
        assert_eq!(page["businessUnitFilterMode"], "subtree");
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
    // Independent prospects and contacts, without organization bindings.
    let account_input = SaveAccount {
        name: "独立潜在客户".into(),
        customer_id: None,
        expected_version: None,
    };
    let account_result = crm
        .save_account(actor, Uuid::new_v4(), None, "account-new", &account_input)
        .await
        .unwrap();
    assert_eq!(
        account_result,
        crm.save_account(actor, Uuid::new_v4(), None, "account-new", &account_input)
            .await
            .unwrap()
    );
    let account_id: Uuid = serde_json::from_value(account_result["id"].clone()).unwrap();
    let mut contact_input = SaveContact {
        account_id,
        name: "陈经理".into(),
        details: "邮箱 A".into(),
        expected_version: None,
    };
    let contact_result = crm
        .save_contact(actor, Uuid::new_v4(), None, "contact-new", &contact_input)
        .await
        .unwrap();
    let contact_id: Uuid = serde_json::from_value(contact_result["id"].clone()).unwrap();
    assert_eq!(
        contact_result,
        crm.save_contact(actor, Uuid::new_v4(), None, "contact-new", &contact_input)
            .await
            .unwrap()
    );
    assert!(
        crm.accounts(outsider, &Filters::default()).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        crm.save_contact(
            outsider,
            Uuid::new_v4(),
            None,
            "contact-denied",
            &contact_input
        )
        .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    input.customer_id = None;
    input.account_id = Some(account_id);
    input.contact_id = Some(contact_id);
    input.stage = "contacting".into();
    input.expected_version = None;
    let mut linked_ids = Vec::new();
    for (index, date) in [
        Some("2026-10-01"),
        Some("2026-10-02"),
        Some("2026-10-09"),
        None,
    ]
    .iter()
    .enumerate()
    {
        input.next_follow_up = date.map(|value| value.parse().unwrap());
        let result = crm
            .save(
                actor,
                Uuid::new_v4(),
                None,
                &format!("directory-opportunity-{index}"),
                &input,
            )
            .await
            .unwrap();
        linked_ids.push(serde_json::from_value::<Uuid>(result["id"].clone()).unwrap());
    }
    for mode in ["overdue", "today", "upcoming", "unscheduled"] {
        let filters = Filters {
            followup: Some(mode.into()),
            today: Some("2026-10-02".parse().unwrap()),
            ..Default::default()
        };
        assert_eq!(
            crm.list(actor, &filters).await.unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "{mode}"
        );
    }
    assert!(matches!(
        crm.list(
            actor,
            &Filters {
                followup: Some("today".into()),
                ..Default::default()
            }
        )
        .await,
        Err(DomainError::Invalid(_))
    ));
    contact_input.expected_version = Some(1);
    contact_input.details = "更新后的邮箱".into();
    crm.save_contact(
        actor,
        Uuid::new_v4(),
        Some(contact_id),
        "contact-edit",
        &contact_input,
    )
    .await
    .unwrap();
    for linked_id in &linked_ids {
        let result = crm.detail(actor, *linked_id, 0).await.unwrap();
        assert_eq!(result["item"]["contactDetails"], "更新后的邮箱");
        assert_eq!(result["item"]["version"], 2);
    }
    assert!(matches!(
        crm.save_contact(
            actor,
            Uuid::new_v4(),
            Some(contact_id),
            "contact-stale",
            &contact_input
        )
        .await,
        Err(DomainError::VersionConflict)
    ));
    let renamed = SaveAccount {
        name: "新客户名称".into(),
        customer_id: None,
        expected_version: Some(1),
    };
    crm.save_account(
        actor,
        Uuid::new_v4(),
        Some(account_id),
        "account-rename",
        &renamed,
    )
    .await
    .unwrap();
    assert_eq!(
        crm.detail(actor, linked_ids[0], 0).await.unwrap()["item"]["companyName"],
        "新客户名称"
    );
    let other = crm
        .save_account(
            actor,
            Uuid::new_v4(),
            None,
            "other-account",
            &SaveAccount {
                name: "另一客户".into(),
                customer_id: None,
                expected_version: None,
            },
        )
        .await
        .unwrap();
    input.account_id = Some(serde_json::from_value(other["id"].clone()).unwrap());
    assert!(matches!(
        crm.save(actor, Uuid::new_v4(), None, "wrong-contact", &input)
            .await,
        Err(DomainError::Invalid(_))
    ));
    // Assignment never grants access. Only an already eligible operator can own the record.
    let owner_scope = business_core::crm::OwnerScope {
        legal_entity_id: legal,
        business_unit_id: unit,
        customer_id: None,
        query: None,
    };
    let owners = crm.owners(actor, &owner_scope).await.unwrap();
    assert_eq!(owners["items"].as_array().unwrap().len(), 1);
    assert!(matches!(
        crm.owners(outsider, &owner_scope).await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    input.account_id = Some(account_id);
    input.expected_version = Some(3);
    input.owner_user_id = Some(outsider);
    input.expected_close_date = Some(Some("2026-11-01".parse().unwrap()));
    assert!(matches!(
        crm.save(
            actor,
            Uuid::new_v4(),
            Some(linked_ids[0]),
            "owner-denied",
            &input
        )
        .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    sqlx::query("INSERT INTO business_legal_entity_scopes(enterprise_user_id,legal_entity_id,granted_by) VALUES($1,$2,$3)").bind(outsider).bind(legal).bind(actor).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$3)").bind(outsider).bind(unit).bind(actor).execute(&pool).await.unwrap();
    assert_eq!(
        crm.owners(actor, &owner_scope).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    crm.save(
        actor,
        Uuid::new_v4(),
        Some(linked_ids[0]),
        "owner-assign",
        &input,
    )
    .await
    .unwrap();
    let assigned = crm.detail(actor, linked_ids[0], 0).await.unwrap();
    assert_eq!(assigned["item"]["ownerUserId"], outsider.to_string());
    assert_eq!(assigned["item"]["ownerName"], "Other User");
    let mine = Filters {
        mine: true,
        ..Default::default()
    };
    assert_eq!(
        crm.list(actor, &mine).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        crm.list(outsider, &mine).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Omitted fields from older clients preserve ownership and date; explicit null clears date.
    input.expected_version = Some(4);
    input.expected_close_date = None;
    input.owner_user_id = None;
    assert!(serde_json::to_value(&input)
        .unwrap()
        .get("expectedCloseDate")
        .is_none());
    crm.save(
        actor,
        Uuid::new_v4(),
        Some(linked_ids[0]),
        "old-client-save",
        &input,
    )
    .await
    .unwrap();
    let preserved = crm.detail(actor, linked_ids[0], 0).await.unwrap();
    assert_eq!(preserved["item"]["expectedCloseDate"], "2026-11-01");
    assert_eq!(preserved["item"]["ownerUserId"], outsider.to_string());
    input.expected_version = Some(5);
    input.expected_close_date = Some(None);
    let roundtrip: SaveOpportunity =
        serde_json::from_value(serde_json::to_value(&input).unwrap()).unwrap();
    assert_eq!(roundtrip.expected_close_date, Some(None));
    crm.save(
        actor,
        Uuid::new_v4(),
        Some(linked_ids[0]),
        "clear-close-date",
        &input,
    )
    .await
    .unwrap();
    assert!(
        crm.detail(actor, linked_ids[0], 0).await.unwrap()["item"]["expectedCloseDate"].is_null()
    );
    input.expected_version = Some(6);
    input.stage = "lost".into();
    input.loss_reason = None;
    assert!(matches!(
        crm.save(
            actor,
            Uuid::new_v4(),
            Some(linked_ids[0]),
            "lost-no-reason",
            &input
        )
        .await,
        Err(DomainError::Invalid(_))
    ));
    input.loss_reason = Some("预算取消".into());
    crm.save(
        actor,
        Uuid::new_v4(),
        Some(linked_ids[0]),
        "lost-with-reason",
        &input,
    )
    .await
    .unwrap();
    let mut outcome = AddFollowup {
        note: "客户反馈".into(),
        stage: "lost".into(),
        next_action: "".into(),
        next_follow_up: None,
        expected_version: 7,
        loss_reason: Some("项目暂停".into()),
    };
    crm.followup(
        actor,
        Uuid::new_v4(),
        linked_ids[0],
        "lost-note-reason",
        &outcome,
    )
    .await
    .unwrap();
    outcome.stage = "contacting".into();
    outcome.expected_version = 8;
    outcome.loss_reason = None;
    crm.followup(
        actor,
        Uuid::new_v4(),
        linked_ids[0],
        "reopen-opportunity",
        &outcome,
    )
    .await
    .unwrap();
    let reopened = crm.detail(actor, linked_ids[0], 0).await.unwrap();
    assert_eq!(reopened["item"]["lossReason"], "");
    assert_eq!(reopened["followups"][1]["lossReason"], "项目暂停");
    // Conversion is atomic, deduplicated and restricted to one opportunity.
    let mut conversion_input = input.clone();
    conversion_input.account_id = None;
    conversion_input.contact_id = None;
    conversion_input.customer_id = None;
    conversion_input.owner_user_id = None;
    conversion_input.expected_version = None;
    conversion_input.stage = "quoting".into();
    conversion_input.company_name = "成交转客户测试".into();
    conversion_input.title = "成交商机".into();
    let conversion_op = crm
        .save(
            actor,
            Uuid::new_v4(),
            None,
            "conversion-op",
            &conversion_input,
        )
        .await
        .unwrap();
    let conversion_id: Uuid = serde_json::from_value(conversion_op["id"].clone()).unwrap();
    let conversion = business_core::crm::ConvertCustomer {
        expected_version: 1,
        customer_id: None,
        customer_name: "成交转客户测试".into(),
        contact_name: "王经理".into(),
        contact_details: "13800138000".into(),
        credit_currency: "CNY".into(),
        payment_terms_days: 30,
        note: "合同已签署".into(),
    };
    assert!(matches!(
        crm.convert_customer(
            actor,
            Uuid::new_v4(),
            conversion_id,
            "conversion-key",
            &conversion
        )
        .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,'business_master_data:manage') ON CONFLICT DO NOTHING").bind(role).execute(&pool).await.unwrap();
    let mut invalid = conversion.clone();
    invalid.payment_terms_days = -1;
    assert!(crm
        .convert_customer(
            actor,
            Uuid::new_v4(),
            conversion_id,
            "conversion-invalid",
            &invalid
        )
        .await
        .is_err());
    assert_eq!(
        crm.detail(actor, conversion_id, 0).await.unwrap()["item"]["stage"],
        "quoting"
    );
    let converted = crm
        .convert_customer(
            actor,
            Uuid::new_v4(),
            conversion_id,
            "conversion-key",
            &conversion,
        )
        .await
        .unwrap();
    assert_eq!(
        converted,
        crm.convert_customer(
            actor,
            Uuid::new_v4(),
            conversion_id,
            "conversion-key",
            &conversion
        )
        .await
        .unwrap()
    );
    assert!(matches!(
        crm.convert_customer(
            actor,
            Uuid::new_v4(),
            conversion_id,
            "conversion-stale",
            &conversion
        )
        .await,
        Err(DomainError::VersionConflict)
    ));
    let detail = crm.detail(actor, conversion_id, 0).await.unwrap();
    assert_eq!(detail["item"]["stage"], "won");
    assert_eq!(detail["item"]["customerId"], converted["customerId"]);
    assert_eq!(detail["followups"][0]["note"], "合同已签署");
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM business_customers WHERE name='成交转客户测试'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM sales_orders")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    let second = crm
        .save(
            actor,
            Uuid::new_v4(),
            None,
            "conversion-second-op",
            &conversion_input,
        )
        .await
        .unwrap();
    let second_id: Uuid = serde_json::from_value(second["id"].clone()).unwrap();
    assert!(matches!(
        crm.convert_customer(
            actor,
            Uuid::new_v4(),
            second_id,
            "conversion-duplicate",
            &conversion
        )
        .await,
        Err(DomainError::Invalid(_))
    ));
    let mut reuse = conversion.clone();
    reuse.customer_id = Some(serde_json::from_value(converted["customerId"].clone()).unwrap());
    let reused = crm
        .convert_customer(actor, Uuid::new_v4(), second_id, "conversion-reuse", &reuse)
        .await
        .unwrap();
    assert_eq!(reused["contactId"], converted["contactId"]);
    assert!(matches!(
        crm.convert_customer(
            outsider,
            Uuid::new_v4(),
            second_id,
            "conversion-outsider",
            &reuse
        )
        .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    // Revoking the business scope hides opportunities but not an owner's independent prospect.
    sqlx::query("DELETE FROM business_unit_scopes WHERE enterprise_user_id=$1")
        .bind(actor)
        .execute(&pool)
        .await
        .unwrap();
    assert!(crm.list(actor, &Filters::default()).await.unwrap()["items"]
        .as_array()
        .unwrap()
        .is_empty());
    let visible_contacts = crm
        .register(actor, &Filters::default(), true)
        .await
        .unwrap();
    assert!(visible_contacts["items"][0]["opportunities"]
        .as_array()
        .unwrap()
        .is_empty());
    // Every CRM route stays behind the existing browser-session middleware.
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    if let Ok(config) = business_core::Config::from_env() {
        let state = business_core::AppState::new(PgStore::new(pool.clone()), &config);
        let router = business_core::router(state);
        for (method, path) in [
            ("GET", "/api/v1/crm/options".to_string()),
            (
                "GET",
                format!("/api/v1/crm/owners?legalEntityId={legal}&businessUnitId={unit}"),
            ),
            ("GET", "/api/v1/crm/opportunities".into()),
            ("GET", "/api/v1/crm/followups".into()),
            ("GET", "/api/v1/crm/contacts".into()),
            ("GET", "/api/v1/crm/accounts".into()),
            ("POST", "/api/v1/crm/accounts".into()),
            ("POST", "/api/v1/crm/contacts".into()),
            ("PUT", format!("/api/v1/crm/accounts/{account_id}")),
            ("PUT", format!("/api/v1/crm/contacts/{contact_id}")),
            ("POST", "/api/v1/crm/opportunities".into()),
            ("PUT", format!("/api/v1/crm/opportunities/{id}")),
            ("POST", format!("/api/v1/crm/opportunities/{id}/followups")),
            (
                "POST",
                format!("/api/v1/crm/opportunities/{id}/convert-customer"),
            ),
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
