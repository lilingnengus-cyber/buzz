use business_core::{
    b2::DomainError,
    master_data::{CoreMasterDataService, CoreMasterType, DeleteCoreMasterData},
    PgStore,
};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

#[tokio::test]
async fn delete_preserves_references_and_enforces_command_contract() {
    let Ok(url) = std::env::var("BUSINESS_CORE_MASTER_TEST_DATABASE_URL") else {
        eprintln!("skipping: BUSINESS_CORE_MASTER_TEST_DATABASE_URL is not set");
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let actor = Uuid::new_v4();
    let outsider = Uuid::new_v4();
    let role = Uuid::new_v4();
    for user in [actor, outsider] {
        sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'delete-test',$2,'Delete test')")
            .bind(user).bind(user.to_string()).execute(&pool).await.unwrap();
    }
    sqlx::query("INSERT INTO business_roles(id,role_key,name) VALUES($1,$2,'Delete test')")
        .bind(role)
        .bind(format!("delete_{}", role.simple()))
        .execute(&pool)
        .await
        .unwrap();
    for permission in ["business_master_data:read", "business_master_data:manage"] {
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,$2)")
            .bind(role)
            .bind(permission)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) VALUES($1,$2,$1)",
    )
    .bind(actor)
    .bind(role)
    .execute(&pool)
    .await
    .unwrap();
    let service = CoreMasterDataService::new(store);
    let legal_id = Uuid::new_v4();
    let unit_id = Uuid::new_v4();
    let legal = Some(legal_id);
    let unit = Some(unit_id);
    let mut records = vec![
        (CoreMasterType::LegalEntity, legal_id, 1),
        (CoreMasterType::BusinessUnit, unit_id, 1),
        (CoreMasterType::Customer, Uuid::new_v4(), 1),
        (CoreMasterType::Supplier, Uuid::new_v4(), 1),
        (CoreMasterType::Warehouse, Uuid::new_v4(), 1),
    ];
    for &(kind, id, _) in &records {
        let (insert, grant) = match kind {
            CoreMasterType::LegalEntity => (
                "INSERT INTO business_legal_entities(id,code,name,country_code,functional_currency) VALUES($1,$2,'Delete test','CN','CNY')",
                "INSERT INTO business_legal_entity_scopes(enterprise_user_id,legal_entity_id,granted_by) VALUES($1,$2,$1)",
            ),
            CoreMasterType::BusinessUnit => (
                "INSERT INTO business_units(id,code,name) VALUES($1,$2,'Delete test')",
                "INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)",
            ),
            CoreMasterType::Customer => (
                "INSERT INTO business_customers(id,code,name,credit_currency) VALUES($1,$2,'Delete test','CNY')",
                "INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)",
            ),
            CoreMasterType::Supplier => (
                "INSERT INTO business_suppliers(id,code,name) VALUES($1,$2,'Delete test')",
                "INSERT INTO business_supplier_scopes(enterprise_user_id,supplier_id,granted_by) VALUES($1,$2,$1)",
            ),
            CoreMasterType::Warehouse => (
                "INSERT INTO business_warehouses(id,code,name) VALUES($1,$2,'Delete test')",
                "INSERT INTO business_warehouse_scopes(enterprise_user_id,warehouse_id,granted_by) VALUES($1,$2,$1)",
            ),
        };
        sqlx::query(insert)
            .bind(id)
            .bind(format!("T{}", &id.simple().to_string()[..16]).to_uppercase())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(grant)
            .bind(actor)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
    }
    // A disabled subordinate in the independent operating tree remains a reference.
    let child = Uuid::new_v4();
    sqlx::query("INSERT INTO business_units(id,code,name,parent_business_unit_id,status) VALUES($1,$2,'Child',$3,'disabled')")
        .bind(child).bind(child.to_string()).bind(unit_id).execute(&pool).await.unwrap();
    assert!(matches!(
        service
            .delete(
                actor,
                Uuid::new_v4(),
                CoreMasterType::BusinessUnit,
                unit_id,
                "blocked-tree",
                &DeleteCoreMasterData {
                    expected_version: 1
                }
            )
            .await,
        Err(DomainError::Invalid(_))
    ));
    sqlx::query("DELETE FROM business_units WHERE id=$1")
        .bind(child)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE business_units SET legal_entity_id=$1 WHERE id=$2")
        .bind(legal_id)
        .bind(unit_id)
        .execute(&pool)
        .await
        .unwrap();
    records[1].2 += 1;
    // Even inactive descendants must prevent deletion of the parent.
    sqlx::query("UPDATE business_units SET status='disabled' WHERE id=$1")
        .bind(unit)
        .execute(&pool)
        .await
        .unwrap();
    records[1].2 += 1;
    let input = DeleteCoreMasterData {
        expected_version: records[0].2,
    };
    assert!(matches!(
        service
            .delete(
                actor,
                Uuid::new_v4(),
                CoreMasterType::LegalEntity,
                legal.unwrap(),
                "blocked-parent",
                &input
            )
            .await,
        Err(DomainError::Invalid(_))
    ));
    // Completed/cancelled historical business still protects a leaf record.
    let receipt = Uuid::new_v4();
    let customer = records[2].1;
    sqlx::query("INSERT INTO customer_receipts(id,receipt_number,legal_entity_id,customer_id,currency,receipt_date,amount,payment_method,status,created_by_user_id,trace_id) VALUES($1,$2,$3,$4,'CNY',CURRENT_DATE,1,'cash','cancelled',$5,$6)")
        .bind(receipt).bind(receipt.to_string()).bind(legal).bind(customer).bind(actor).bind(Uuid::new_v4())
        .execute(&pool).await.unwrap();
    assert!(matches!(
        service
            .delete(
                actor,
                Uuid::new_v4(),
                CoreMasterType::Customer,
                customer,
                "historical-reference",
                &DeleteCoreMasterData {
                    expected_version: records[2].2
                }
            )
            .await,
        Err(DomainError::Invalid(_))
    ));
    let scoped: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2)")
        .bind(actor).bind(customer).fetch_one(&pool).await.unwrap();
    assert!(scoped, "failed deletion must roll back scope cascades");
    // Remove only this disposable fixture so the rest of the delete matrix can run.
    sqlx::query("DELETE FROM customer_receipts WHERE id=$1")
        .bind(receipt)
        .execute(&pool)
        .await
        .unwrap();
    for (kind, id, version) in records.into_iter().rev() {
        let input = DeleteCoreMasterData {
            expected_version: version,
        };
        assert!(matches!(
            service
                .delete(outsider, Uuid::new_v4(), kind, id, "forbidden", &input)
                .await,
            Err(DomainError::NotFoundOrForbidden)
        ));
        // A management permission alone does not grant access to the object's scope.
        sqlx::query("INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) VALUES($1,$2,$1) ON CONFLICT DO NOTHING")
            .bind(outsider).bind(role).execute(&pool).await.unwrap();
        assert!(matches!(
            service
                .delete(outsider, Uuid::new_v4(), kind, id, "outside-scope", &input)
                .await,
            Err(DomainError::NotFoundOrForbidden)
        ));
        assert!(matches!(
            service
                .delete(
                    actor,
                    Uuid::new_v4(),
                    kind,
                    id,
                    "stale-version",
                    &DeleteCoreMasterData {
                        expected_version: version + 1
                    }
                )
                .await,
            Err(DomainError::VersionConflict)
        ));
        let key = Uuid::new_v4().to_string();
        let deleted = service
            .delete(actor, Uuid::new_v4(), kind, id, &key, &input)
            .await
            .unwrap();
        assert_eq!(deleted.status, "deleted");
        assert!(!service
            .list(actor, None, 1000)
            .await
            .unwrap()
            .items
            .iter()
            .any(|item| item.id == id));
        let replay = service
            .delete(actor, Uuid::new_v4(), kind, id, &key, &input)
            .await
            .unwrap();
        assert!(replay.idempotent_replay);
        assert_eq!(deleted.trace_id, replay.trace_id);
        assert!(matches!(
            service
                .delete(
                    actor,
                    Uuid::new_v4(),
                    kind,
                    id,
                    &key,
                    &DeleteCoreMasterData {
                        expected_version: version + 1
                    }
                )
                .await,
            Err(DomainError::IdempotencyConflict)
        ));
        assert!(matches!(
            service
                .delete(actor, Uuid::new_v4(), kind, id, "missing-record", &input)
                .await,
            Err(DomainError::NotFoundOrForbidden)
        ));
    }
    let audit_count: i64 = sqlx::query_scalar("SELECT count(*) FROM business_core_audit_events WHERE actor_user_id=$1 AND operation='CORE_MASTER_DATA_DELETED'")
        .bind(actor).fetch_one(&pool).await.unwrap();
    assert_eq!(audit_count, 5);
    let outbox_count: i64 = sqlx::query_scalar("SELECT count(*) FROM business_core_outbox o JOIN business_core_audit_events a ON a.target_id=o.aggregate_id WHERE a.actor_user_id=$1 AND a.operation='CORE_MASTER_DATA_DELETED' AND o.topic='core_master_data_deleted'")
        .bind(actor).fetch_one(&pool).await.unwrap();
    assert_eq!(outbox_count, 5);
}
