use super::*;
use business_core::model::ResourceType;

pub(super) async fn check(store: &PgStore, fixture: &Fixture) {
    let pool = store.pool();
    let catalog_count: i64 = sqlx::query_scalar("SELECT count(*) FROM business_iam.permissions WHERE capability='business_master_data:read'").fetch_one(pool).await.unwrap();
    assert_eq!(catalog_count, 1);

    sqlx::query("INSERT INTO business_customers(id,legal_entity_id,business_unit_id,code,name,credit_currency,payment_terms_days) SELECT gen_random_uuid(),$1,$2,'A_SEARCH_'||n,'查找同名客户','CNY',30 FROM generate_series(1,205) n")
        .bind(fixture.legal_entity).bind(fixture.business_unit).execute(pool).await.unwrap();
    let mut ids = Vec::new();
    for code in ["Z_SEARCH_1", "Z_SEARCH_2"] {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO business_customers(id,legal_entity_id,business_unit_id,code,name,credit_currency,payment_terms_days) VALUES($1,$2,$3,$4,'查找同名客户','CNY',30)")
            .bind(id).bind(fixture.legal_entity).bind(fixture.business_unit).bind(code).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)")
            .bind(fixture.actor).bind(id).execute(pool).await.unwrap();
        ids.push(id);
    }
    let snapshot = store.snapshot(fixture.actor).await.unwrap();
    let first = store
        .search_resources(
            ResourceType::Customer,
            &snapshot,
            "同名",
            Some(fixture.legal_entity),
            0,
            1,
        )
        .await
        .unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].id, ids[0]);
    let next = store
        .search_resources(ResourceType::Customer, &snapshot, "同名", None, 1, 1)
        .await
        .unwrap();
    assert_eq!(next[0].id, ids[1]);
    let duplicates = store
        .search_resources(ResourceType::Customer, &snapshot, "同名", None, 0, 20)
        .await
        .unwrap();
    assert_eq!(duplicates.len(), 2);
    let literal = store
        .search_resources(ResourceType::Customer, &snapshot, "%", None, 0, 20)
        .await
        .unwrap();
    assert!(literal.is_empty());
    let wrong_entity = store
        .search_resources(
            ResourceType::Customer,
            &snapshot,
            "同名",
            Some(Uuid::new_v4()),
            0,
            20,
        )
        .await
        .unwrap();
    assert!(wrong_entity.is_empty());
    // Restore the fixture's permissions for the remaining closed-loop tests.
    for id in ids {
        sqlx::query(
            "DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2",
        )
        .bind(fixture.actor)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }
}
