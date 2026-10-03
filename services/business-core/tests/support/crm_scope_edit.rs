use business_core::{
    b2::DomainError,
    crm::{CrmService, SaveOpportunity},
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
    // Operating-unit edits require access to both scopes and an eligible owner.
    let target_unit = Uuid::new_v4();
    sqlx::query("INSERT INTO business_units(id,legal_entity_id,code,name) VALUES($1,$2,'CRM_TARGET','目标经营单元')")
        .bind(target_unit)
        .bind(legal)
        .execute(pool)
        .await
        .unwrap();
    let mut moving: SaveOpportunity = serde_json::from_value(serde_json::json!({
        "legalEntityId": legal, "businessUnitId": unit,
        "title": "经营范围修改测试", "companyName": "独立客户",
        "stage": "new", "currency": "CNY"
    }))
    .unwrap();
    let saved = crm
        .save(actor, Uuid::new_v4(), None, "move-create", &moving)
        .await
        .unwrap();
    let moving_id = serde_json::from_value(saved["id"].clone()).unwrap();
    moving.expected_version = Some(1);
    moving.business_unit_id = target_unit;
    assert!(matches!(
        crm.save(
            actor,
            Uuid::new_v4(),
            Some(moving_id),
            "move-denied",
            &moving
        )
        .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)")
        .bind(actor).bind(target_unit).execute(pool).await.unwrap();
    moving.owner_user_id = Some(outsider);
    assert!(matches!(
        crm.save(
            actor,
            Uuid::new_v4(),
            Some(moving_id),
            "move-owner-denied",
            &moving
        )
        .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    moving.owner_user_id = None;
    let moved = crm
        .save(
            actor,
            Uuid::new_v4(),
            Some(moving_id),
            "move-allowed",
            &moving,
        )
        .await
        .unwrap();
    assert_eq!(moved["version"], 2);
    assert_eq!(
        crm.detail(actor, moving_id, 0).await.unwrap()["item"]["businessUnitId"],
        target_unit.to_string()
    );
    assert!(matches!(
        crm.detail(outsider, moving_id, 0).await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    assert!(matches!(
        crm.save(
            actor,
            Uuid::new_v4(),
            Some(moving_id),
            "move-stale",
            &moving
        )
        .await,
        Err(DomainError::VersionConflict)
    ));
    moving.business_unit_id = unit;
    moving.expected_version = Some(2);
    sqlx::query(
        "DELETE FROM business_unit_scopes WHERE enterprise_user_id=$1 AND business_unit_id=$2",
    )
    .bind(actor)
    .bind(target_unit)
    .execute(pool)
    .await
    .unwrap();
    assert!(matches!(
        crm.save(
            actor,
            Uuid::new_v4(),
            Some(moving_id),
            "move-old-denied",
            &moving
        )
        .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
}
