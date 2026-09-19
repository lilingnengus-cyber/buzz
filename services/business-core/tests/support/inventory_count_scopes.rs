use super::{version, Fixture};
use business_core::{
    b2::{CreateInventoryCount, DomainError, InventoryCountService, SubmitInventoryCount},
    PgStore,
};
use uuid::Uuid;

pub(super) async fn check(
    store: &PgStore,
    service: &InventoryCountService,
    f: &Fixture,
    input: &CreateInventoryCount,
    posted: Uuid,
    cancelled: Uuid,
    submitted: &SubmitInventoryCount,
) {
    for (table, revoke, restore, value) in [
("business_unit_scopes", "DELETE FROM business_unit_scopes WHERE enterprise_user_id=$1 AND business_unit_id=$2", "INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)", f.business_unit),
("business_brand_scopes", "DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2", "INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)", f.brand),
("business_warehouse_scopes", "DELETE FROM business_warehouse_scopes WHERE enterprise_user_id=$1 AND warehouse_id=$2", "INSERT INTO business_warehouse_scopes(enterprise_user_id,warehouse_id,granted_by) VALUES($1,$2,$1)", f.warehouse),
("business_legal_entity_scopes", "DELETE FROM business_legal_entity_scopes WHERE enterprise_user_id=$1 AND legal_entity_id=$2", "INSERT INTO business_legal_entity_scopes(enterprise_user_id,legal_entity_id,granted_by) VALUES($1,$2,$1)", f.legal_entity)
] {
        assert!(!service.options(f.actor).await.unwrap().is_empty());
        assert_eq!(service.list(f.actor, 500).await.unwrap().len(), 3);
        sqlx::query(revoke)
        .bind(f.actor)
        .bind(value)
        .execute(store.pool())
        .await
        .unwrap();
        assert!(
            service.options(f.actor).await.unwrap().is_empty(),
            "{table}"
        );
        assert!(
            service.list(f.actor, 500).await.unwrap().is_empty(),
            "{table}"
        );
        assert!(
            matches!(
                service.detail(f.actor, posted).await,
                Err(DomainError::NotFoundOrForbidden)
            ),
            "{table}"
        );
        for key in ["count-create-first", "count-create-revoked"] {
            assert!(
                matches!(
                    service.create(f.actor, Uuid::new_v4(), key, input).await,
                    Err(DomainError::NotFoundOrForbidden)
                ),
                "{table}: {key}"
            );
        }
        for key in ["count-submit-first", "count-submit-revoked"] {
            assert!(
                matches!(
                    service
                        .submit(f.actor, Uuid::new_v4(), posted, key, submitted)
                        .await,
                    Err(DomainError::NotFoundOrForbidden)
                ),
                "{table}: {key}"
            );
        }
        for key in ["count-post-shared", "count-post-revoked"] {
            assert!(
                matches!(
                    service
                        .post(f.actor, Uuid::new_v4(), posted, key, &version(2))
                        .await,
                    Err(DomainError::NotFoundOrForbidden)
                ),
                "{table}: {key}"
            );
        }
        for key in ["count-cancel-shared", "count-cancel-revoked"] {
            assert!(
                matches!(
                    service
                        .cancel(f.actor, Uuid::new_v4(), cancelled, key, &version(2))
                        .await,
                    Err(DomainError::NotFoundOrForbidden)
                ),
                "{table}: {key}"
            );
        }
        sqlx::query(restore)
        .bind(f.actor)
        .bind(value)
        .execute(store.pool())
        .await
        .unwrap();
    }
    assert_eq!(
        service.detail(f.actor, posted).await.unwrap().status,
        "posted"
    );
    assert_eq!(
        service.detail(f.actor, cancelled).await.unwrap().status,
        "cancelled"
    );
}
