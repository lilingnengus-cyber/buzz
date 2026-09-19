use super::{version, Fixture};
use business_core::{
    b2::{CreateInventoryCount, DomainError, InventoryCountService},
    PgStore,
};
use sqlx::Row;
use uuid::Uuid;

pub(super) async fn check(
    store: &PgStore,
    service: &InventoryCountService,
    f: &Fixture,
    input: &CreateInventoryCount,
    id: Uuid,
) {
    let row=sqlx::query("SELECT scope_snapshot_captured,snapshot_business_unit_id FROM inventory_count_tasks WHERE id=$1").bind(id).fetch_one(store.pool()).await.unwrap();
    assert!(row.get::<bool, _>("scope_snapshot_captured"));
    assert_eq!(
        row.get::<Option<Uuid>, _>("snapshot_business_unit_id"),
        Some(f.business_unit)
    );
    let brand: Option<Uuid> = sqlx::query_scalar(
        "SELECT snapshot_brand_id FROM inventory_count_lines WHERE inventory_count_id=$1",
    )
    .bind(id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(brand, Some(f.brand));
    assert!(sqlx::query(
        "UPDATE inventory_count_tasks SET scope_snapshot_captured=false WHERE id=$1"
    )
    .bind(id)
    .execute(store.pool())
    .await
    .is_err());
    assert!(sqlx::query(
        "UPDATE inventory_count_lines SET snapshot_brand_id=NULL WHERE inventory_count_id=$1"
    )
    .bind(id)
    .execute(store.pool())
    .await
    .is_err());
    let new_brand = Uuid::new_v4();
    let new_unit = Uuid::new_v4();
    sqlx::query("INSERT INTO business_brands(id,code,name) VALUES($1,'COUNT_NEW_BRAND','Changed count brand')").bind(new_brand).execute(store.pool()).await.unwrap();
    sqlx::query("INSERT INTO business_units(id,legal_entity_id,code,name) VALUES($1,$2,'COUNT_NEW_UNIT','Changed count unit')").bind(new_unit).bind(f.legal_entity).execute(store.pool()).await.unwrap();
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(new_brand).execute(store.pool()).await.unwrap();
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(new_unit).execute(store.pool()).await.unwrap();
    sqlx::query("UPDATE business_products SET brand_id=$2 WHERE id=(SELECT product_id FROM business_skus WHERE id=$1)").bind(f.sku).bind(new_brand).execute(store.pool()).await.unwrap();
    sqlx::query("UPDATE business_warehouses SET business_unit_id=$2 WHERE id=$1")
        .bind(f.warehouse)
        .bind(new_unit)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(service.detail(f.actor, id).await.is_ok());
    for (revoke,restore,value) in [
        ("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2","INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)",f.brand),
        ("DELETE FROM business_unit_scopes WHERE enterprise_user_id=$1 AND business_unit_id=$2","INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)",f.business_unit),
        ("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2","INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)",new_brand),
        ("DELETE FROM business_unit_scopes WHERE enterprise_user_id=$1 AND business_unit_id=$2","INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)",new_unit),
    ] {
        sqlx::query(revoke).bind(f.actor).bind(value).execute(store.pool()).await.unwrap();
        assert!(service.list(f.actor,500).await.unwrap().is_empty());
        assert!(matches!(service.detail(f.actor,id).await,Err(DomainError::NotFoundOrForbidden)));
        assert!(matches!(service.create(f.actor,Uuid::new_v4(),"count-create-first",input).await,Err(DomainError::NotFoundOrForbidden)));
        assert!(matches!(service.post(f.actor,Uuid::new_v4(),id,"count-post-shared",&version(2)).await,Err(DomainError::NotFoundOrForbidden)));
        sqlx::query(restore).bind(f.actor).bind(value).execute(store.pool()).await.unwrap();
    }
    assert!(
        service
            .create(f.actor, Uuid::new_v4(), "count-create-first", input)
            .await
            .unwrap()
            .idempotent_replay
    );
    sqlx::query("UPDATE business_products SET brand_id=$2 WHERE id=(SELECT product_id FROM business_skus WHERE id=$1)").bind(f.sku).bind(f.brand).execute(store.pool()).await.unwrap();
    sqlx::query("UPDATE business_warehouses SET business_unit_id=$2 WHERE id=$1")
        .bind(f.warehouse)
        .bind(f.business_unit)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM business_brand_scopes WHERE brand_id=$1")
        .bind(new_brand)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM business_unit_scopes WHERE business_unit_id=$1")
        .bind(new_unit)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM business_brands WHERE id=$1")
        .bind(new_brand)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM business_units WHERE id=$1")
        .bind(new_unit)
        .execute(store.pool())
        .await
        .unwrap();
}
