use business_core::{
    b2::DomainError,
    crm::{AddFollowup, CrmService},
    PgStore,
};
use business_query_contracts::{GetCrmOpportunityInput, SearchCrmOpportunitiesInput};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
fn search(value: Value) -> SearchCrmOpportunitiesInput {
    serde_json::from_value(value).unwrap()
}
fn detail(id: Uuid, offset: u32, version: Option<i64>) -> GetCrmOpportunityInput {
    serde_json::from_value(json!({"documentId":id,"offset":offset,"expectedVersion":version}))
        .unwrap()
}
pub async fn check(pool: &PgPool, actor: Uuid, outsider: Uuid) {
    let crm = CrmService::new(PgStore::new(pool.clone()));
    let found = crm
        .agent_search(
            actor,
            search(json!({"query":"Updated by approved CRM intent","stage":"won"})),
        )
        .await
        .unwrap();
    assert_eq!(found["items"].as_array().unwrap().len(), 1);
    assert_eq!(found["hasMore"], false);
    let id: Uuid = serde_json::from_value(found["items"][0]["id"].clone()).unwrap();
    assert_eq!(found["items"][0]["version"], 3);
    for version in 3..8 {
        crm.followup(
            actor,
            Uuid::new_v4(),
            id,
            &format!("crm-history-page-{version}"),
            &AddFollowup {
                note: "中".repeat(4000),
                stage: "won".into(),
                next_action: "Next".into(),
                next_follow_up: None,
                expected_version: version,
            },
        )
        .await
        .unwrap();
    }
    let first = crm.agent_detail(actor, detail(id, 0, None)).await.unwrap();
    assert_eq!(first["item"]["version"], 8);
    assert_eq!(first["hasMore"], true);
    assert_eq!(first["nextOffset"], 3);
    assert!(serde_json::to_vec(&first).unwrap().len() < 50 * 1024);
    let second = crm
        .agent_detail(actor, detail(id, 3, Some(8)))
        .await
        .unwrap();
    assert_eq!(second["hasMore"], false);
    let ids = first["followups"]
        .as_array()
        .unwrap()
        .iter()
        .chain(second["followups"].as_array().unwrap())
        .map(|row| row["id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 6);
    let page = crm
        .agent_search(actor, search(json!({"limit":1})))
        .await
        .unwrap();
    assert_eq!(page["hasMore"], true);
    assert_eq!(page["nextOffset"], 1);
    let next = crm
        .agent_search(actor, search(json!({"limit":1,"offset":1})))
        .await
        .unwrap();
    assert_eq!(next["hasMore"], false);
    assert_ne!(page["items"][0]["id"], next["items"][0]["id"]);
    assert!(
        crm.agent_search(outsider, search(json!({}))).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        crm.agent_detail(outsider, detail(id, 0, None)).await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    assert!(crm
        .agent_search(actor, search(json!({"documentId":id,"stage":"new"})))
        .await
        .unwrap()["items"]
        .as_array()
        .unwrap()
        .is_empty());
    crm.followup(
        actor,
        Uuid::new_v4(),
        id,
        "crm-history-changed",
        &AddFollowup {
            note: "New note".into(),
            stage: "won".into(),
            next_action: "Next".into(),
            next_follow_up: None,
            expected_version: 8,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        crm.agent_detail(actor, detail(id, 3, Some(8))).await,
        Err(DomainError::VersionConflict)
    ));
}
