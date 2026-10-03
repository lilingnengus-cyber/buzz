use business_core::{
    b2::DomainError,
    crm::{ConvertLead, CrmService, Filters, LeadFilters, LeadFollowup, SaveLead, SaveOpportunity},
};
use sqlx::PgPool;
use uuid::Uuid;
pub async fn check(
    pool: &PgPool,
    crm: &CrmService,
    actor: Uuid,
    outsider: Uuid,
    legal: Uuid,
    unit: Uuid,
) {
    let trace = Uuid::new_v4();
    let customer_count: i64 = sqlx::query_scalar("SELECT count(*) FROM business_customers")
        .fetch_one(pool)
        .await
        .unwrap();
    let input = SaveLead {
        title: "线索筛选验收".into(),
        company_name: "候选公司".into(),
        summary: "需".repeat(4000),
        ..Default::default()
    };
    let created = crm
        .save_lead(actor, trace, None, "lead-create", &input)
        .await
        .unwrap();
    assert_eq!(
        created,
        crm.save_lead(actor, trace, None, "lead-create", &input)
            .await
            .unwrap()
    );
    let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    assert!(matches!(
        crm.lead_detail(outsider, id, 0).await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    assert!(
        crm.leads(outsider, &LeadFilters::default()).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let delegated = SaveLead {
        title: "交给其他负责人".into(),
        owner_user_id: Some(outsider),
        ..Default::default()
    };
    let assigned = crm
        .save_lead(actor, trace, None, "lead-assigned", &delegated)
        .await
        .unwrap();
    let assigned_id: Uuid = serde_json::from_value(assigned["id"].clone()).unwrap();
    assert!(crm.lead_detail(outsider, assigned_id, 0).await.is_ok());
    assert!(crm.lead_detail(actor, assigned_id, 0).await.is_ok());
    let owned = crm
        .leads(
            actor,
            &LeadFilters {
                owner_user_id: Some(outsider),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(owned["items"].as_array().unwrap().len(), 1);
    assert_eq!(owned["items"][0]["id"], assigned["id"]);
    let stale = SaveLead {
        title: "旧版本修改".into(),
        expected_version: Some(99),
        ..Default::default()
    };
    assert!(matches!(
        crm.save_lead(actor, trace, Some(id), "lead-stale", &stale)
            .await,
        Err(DomainError::VersionConflict)
    ));
    let duplicate = crm
        .save_lead(actor, trace, None, "lead-duplicate", &input)
        .await
        .unwrap();
    assert_eq!(
        crm.lead_detail(actor, id, 0).await.unwrap()["duplicates"][0]["id"],
        duplicate["id"]
    );
    let mut note = LeadFollowup {
        note: "记".repeat(4000),
        status: "disqualified".into(),
        next_action: "".into(),
        next_follow_up: None,
        disqualification_reason: "".into(),
        expected_version: 1,
    };
    assert!(crm
        .lead_followup(actor, trace, id, "lead-invalid", &note)
        .await
        .is_err());
    note.disqualification_reason = "需求不明确".into();
    crm.lead_followup(actor, trace, id, "lead-disqualify", &note)
        .await
        .unwrap();
    let make_opportunity = || SaveOpportunity {
        legal_entity_id: legal,
        business_unit_id: unit,
        customer_id: None,
        account_id: None,
        contact_id: None,
        title: "合格线索".into(),
        company_name: "候选公司".into(),
        contact_name: "张经理".into(),
        contact_details: "".into(),
        stage: "contacting".into(),
        expected_amount_minor: None,
        currency: "CNY".into(),
        next_action: "发送方案".into(),
        next_follow_up: None,
        expected_version: None,
        owner_user_id: Some(actor),
        expected_close_date: None,
        loss_reason: None,
    };
    let mut conversion = ConvertLead {
        expected_version: 2,
        opportunity: make_opportunity(),
    };
    assert!(crm
        .convert_lead(actor, trace, id, "lead-blocked", &conversion)
        .await
        .is_err());
    note.status = "contacting".into();
    note.disqualification_reason.clear();
    note.expected_version = 2;
    crm.lead_followup(actor, trace, id, "lead-reopen", &note)
        .await
        .unwrap();
    let register = crm
        .register(
            actor,
            &Filters {
                query: Some("线索筛选验收".into()),
                ..Default::default()
            },
            false,
        )
        .await
        .unwrap();
    assert_eq!(register["items"].as_array().unwrap().len(), 2);
    assert_eq!(register["items"][0]["leadId"], created["id"]);
    conversion.expected_version = 3;
    conversion.opportunity.business_unit_id = Uuid::new_v4();
    assert!(crm
        .convert_lead(actor, trace, id, "lead-no-scope", &conversion)
        .await
        .is_err());
    assert_eq!(
        crm.lead_detail(actor, id, 0).await.unwrap()["item"]["status"],
        "contacting"
    );
    conversion.opportunity.business_unit_id = unit;
    let result = crm
        .convert_lead(actor, trace, id, "lead-convert", &conversion)
        .await
        .unwrap();
    assert_eq!(
        result,
        crm.convert_lead(actor, trace, id, "lead-convert", &conversion)
            .await
            .unwrap()
    );
    assert!(crm
        .convert_lead(actor, trace, id, "lead-convert-again", &conversion)
        .await
        .is_err());
    let opportunity: Uuid = serde_json::from_value(result["id"].clone()).unwrap();
    let detail = crm.detail(actor, opportunity, 0).await.unwrap();
    assert_eq!(detail["sourceLeadId"], created["id"]);
    assert_eq!(detail["followups"].as_array().unwrap().len(), 3);
    assert!(detail["followups"]
        .as_array()
        .unwrap()
        .iter()
        .all(|n| n["note"].as_str().unwrap().chars().count() == 4000));
    assert_eq!(
        customer_count,
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM business_customers")
            .fetch_one(pool)
            .await
            .unwrap()
    );
    assert!(crm
        .lead_followup(actor, trace, id, "lead-after", &note)
        .await
        .is_err());
    assert_eq!(
        crm.lead_detail(actor, id, 0).await.unwrap()["item"]["convertedOpportunityId"],
        result["id"]
    );
    let register = crm
        .register(
            actor,
            &Filters {
                query: Some("合格线索".into()),
                ..Default::default()
            },
            false,
        )
        .await
        .unwrap();
    assert_eq!(register["items"].as_array().unwrap().len(), 3);
    assert!(register["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|n| n["sourceLeadId"] == created["id"]));
    // Remove crm:manage temporarily: read-only users cannot record or convert leads.
    sqlx::query("DELETE FROM business_role_permissions WHERE permission_key='crm:manage'")
        .execute(pool)
        .await
        .unwrap();
    assert!(crm
        .save_lead(actor, trace, None, "lead-readonly", &input)
        .await
        .is_err());
}
