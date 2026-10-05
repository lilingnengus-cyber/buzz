use business_core::crm::{ConvertCustomer, CrmService, SaveOpportunity};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn check(
    crm: &CrmService,
    pool: &PgPool,
    actor: Uuid,
    base: &SaveOpportunity,
    conversion: &ConvertCustomer,
) {
    let mut input = base.clone();
    input.company_name = "联系人归档回归".into();
    input.contact_name = conversion.contact_name.clone();
    input.contact_details = conversion.contact_details.clone();
    let mut ids = Vec::new();
    for key in ["contact-shared-first", "contact-shared-second"] {
        let result = crm
            .save(actor, Uuid::new_v4(), None, key, &input)
            .await
            .unwrap();
        ids.push(serde_json::from_value::<Uuid>(result["id"].clone()).unwrap());
    }
    let before = crm.detail(actor, ids[0], 0).await.unwrap();
    let source: Uuid = serde_json::from_value(before["item"]["contactId"].clone()).unwrap();
    let first = crm
        .convert_customer(
            actor,
            Uuid::new_v4(),
            ids[0],
            "contact-convert-first",
            conversion,
        )
        .await
        .unwrap();
    assert!(first["retiredContactId"].is_null());
    let other = crm.detail(actor, ids[1], 0).await.unwrap();
    assert_eq!(other["item"]["contactId"], source.to_string());
    assert_eq!(other["item"]["version"], 1);
    assert_eq!(other["item"]["stage"], "quoting");
    let second = crm
        .convert_customer(
            actor,
            Uuid::new_v4(),
            ids[1],
            "contact-convert-second",
            conversion,
        )
        .await
        .unwrap();
    assert_eq!(first["contactId"], second["contactId"]);
    assert_eq!(second["retiredContactId"], source.to_string());
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM crm_contacts WHERE id=$1)")
        .bind(source)
        .fetch_one(pool)
        .await
        .unwrap();
    assert!(
        !exists,
        "last conversion must not leave an orphan duplicate"
    );
    assert_eq!(
        second,
        crm.convert_customer(
            actor,
            Uuid::new_v4(),
            ids[1],
            "contact-convert-second",
            conversion
        )
        .await
        .unwrap()
    );

    // A different confirmed person must not erase the original directory entry.
    input.company_name = "联系人变更回归".into();
    input.contact_name = "原对接人".into();
    let created = crm
        .save(actor, Uuid::new_v4(), None, "contact-changed", &input)
        .await
        .unwrap();
    let id = serde_json::from_value(created["id"].clone()).unwrap();
    let before = crm.detail(actor, id, 0).await.unwrap();
    let source: Uuid = serde_json::from_value(before["item"]["contactId"].clone()).unwrap();
    let result = crm
        .convert_customer(
            actor,
            Uuid::new_v4(),
            id,
            "contact-changed-convert",
            conversion,
        )
        .await
        .unwrap();
    assert!(result["retiredContactId"].is_null());
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM crm_contacts WHERE id=$1)")
            .bind(source)
            .fetch_one(pool)
            .await
            .unwrap()
    );
}
