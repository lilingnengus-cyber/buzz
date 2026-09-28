use business_core::{b2::DomainError, user_preferences::UserPreferenceService, PgStore};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

#[tokio::test]
async fn operating_unit_preferences_follow_the_user_and_current_scope() {
    let Ok(database_url) = std::env::var("BUSINESS_CORE_TEST_DATABASE_URL") else {
        eprintln!("skipping: BUSINESS_CORE_TEST_DATABASE_URL is not set");
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .unwrap();
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let preferences = UserPreferenceService::new(store);
    let actor = Uuid::new_v4();
    let outsider = Uuid::new_v4();
    let legal_entity = Uuid::new_v4();
    let unit = Uuid::new_v4();
    let alternate_unit = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name)
         VALUES($1,'preference-test',$1::text,'Preference User'),
               ($2,'preference-test',$2::text,'Preference Outsider')",
    )
    .bind(actor)
    .bind(outsider)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO business_legal_entities(
            id,code,name,country_code,functional_currency
         ) VALUES($1,$2,'Preference Legal Entity','CN','CNY')",
    )
    .bind(legal_entity)
    .bind(format!(
        "PREF_LE_{}",
        &legal_entity.simple().to_string()[..8]
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO business_units(id,legal_entity_id,code,name)
         VALUES($1,$2,$3,'Preference Unit')",
    )
    .bind(unit)
    .bind(legal_entity)
    .bind(format!("PREF_BU_{}", &unit.simple().to_string()[..8]))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO business_units(id,legal_entity_id,code,name)
         VALUES($1,$2,$3,'Alternate Preference Unit')",
    )
    .bind(alternate_unit)
    .bind(legal_entity)
    .bind(format!(
        "PREF_BU_{}",
        &alternate_unit.simple().to_string()[..8]
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO business_unit_scopes(
            enterprise_user_id,business_unit_id,granted_by
         ) VALUES($1,$2,$1),($1,$3,$1)",
    )
    .bind(actor)
    .bind(unit)
    .bind(alternate_unit)
    .execute(&pool)
    .await
    .unwrap();

    preferences
        .save_operating_unit(actor, "sales-order", unit, false)
        .await
        .unwrap();
    assert_eq!(
        preferences
            .operating_unit(actor, "sales-order")
            .await
            .unwrap(),
        Some(unit)
    );
    assert_eq!(
        preferences
            .operating_unit(actor, "purchase-order")
            .await
            .unwrap(),
        None
    );
    preferences
        .save_operating_unit(actor, "sales-order", unit, true)
        .await
        .unwrap();
    preferences
        .save_operating_unit(actor, "sales-order", alternate_unit, false)
        .await
        .unwrap();
    let pinned = preferences
        .operating_unit_preference(actor, "sales-order")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pinned.business_unit_id, unit);
    assert!(pinned.pinned);
    preferences
        .clear_operating_unit(actor, "sales-order")
        .await
        .unwrap();
    assert_eq!(
        preferences
            .operating_unit(actor, "sales-order")
            .await
            .unwrap(),
        None
    );
    preferences
        .save_operating_unit(actor, "sales-order", unit, false)
        .await
        .unwrap();
    assert!(matches!(
        preferences
            .save_operating_unit(outsider, "sales-order", unit, false)
            .await,
        Err(DomainError::NotFoundOrForbidden)
    ));

    sqlx::query("UPDATE business_units SET status='disabled' WHERE id=$1")
        .bind(unit)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        preferences
            .operating_unit(actor, "sales-order")
            .await
            .unwrap(),
        None
    );
}
