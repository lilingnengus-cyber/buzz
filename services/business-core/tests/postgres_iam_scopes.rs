use business_core::{
    crm::{CrmService, Filters, OwnerScope, SaveOpportunity},
    master_data::{CoreMasterDataService, CoreMasterType},
    PgStore,
};
use serde_json::{json, Value};
use sqlx::{postgres::PgPoolOptions, PgPool};
use uuid::Uuid;

async fn grant(pool: &PgPool, role: Uuid, capability: &str, scope: Value) {
    sqlx::query("INSERT INTO business_iam.role_permissions(role_id,permission_id,data_scope) SELECT $1,id,$3 FROM business_iam.permissions WHERE capability=$2 ON CONFLICT(role_id,permission_id) DO UPDATE SET data_scope=EXCLUDED.data_scope")
        .bind(role).bind(capability).bind(scope).execute(pool).await.unwrap();
}
fn restricted(unit: Uuid) -> Value {
    json!({"mode":"restricted","dimensions":{"business_unit":[unit]}})
}

#[tokio::test]
async fn iam_only_presales_role_enforces_exact_units_and_live_revocation() {
    let Ok(url) = std::env::var("BUSINESS_CORE_IAM_TEST_DATABASE_URL") else {
        eprintln!("skipping: BUSINESS_CORE_IAM_TEST_DATABASE_URL unset");
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let user = Uuid::new_v4();
    let principal = Uuid::new_v4();
    let role = Uuid::new_v4();
    let legal = Uuid::new_v4();
    let unit = Uuid::new_v4();
    let child = Uuid::new_v4();
    let outside = Uuid::new_v4();
    let customer = Uuid::new_v4();
    let other_customer = Uuid::new_v4();
    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'iam-scope-test',$1::text,'Scoped salesperson')").bind(user).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_legal_entities(id,code,name,country_code,functional_currency) VALUES($1,'IAM_LE','IAM Legal','CN','CNY')").bind(legal).execute(&pool).await.unwrap();
    for (id, code, parent) in [
        (unit, "IAM_UNIT", None),
        (child, "IAM_CHILD", Some(unit)),
        (outside, "IAM_OTHER", None),
    ] {
        sqlx::query(
            "INSERT INTO business_units(id,code,name,parent_business_unit_id,legal_entity_id) VALUES($1,$2,$2,$3,$4)",
        )
        .bind(id)
        .bind(code)
        .bind(parent)
        .bind(legal)
        .execute(&pool)
        .await
        .unwrap();
    }
    for (id, code) in [(customer, "IAM_C1"), (other_customer, "IAM_C2")] {
        sqlx::query(
            "INSERT INTO business_customers(id,code,name,credit_currency,legal_entity_id,business_unit_id) VALUES($1,$2,$2,'CNY',$3,$4)",
        )
        .bind(id)
        .bind(code)
        .bind(legal)
        .bind(outside)
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::query("INSERT INTO business_iam.principals(id,kind,external_id,display_name) VALUES($1,'human',$2,'Scoped salesperson')").bind(principal).bind(user.to_string()).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_iam.roles(id,code,name) VALUES($1,'iam.test.salesperson','Scoped salesperson')").bind(role).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_iam.principal_roles(principal_id,role_id) VALUES($1,$2)")
        .bind(principal)
        .bind(role)
        .execute(&pool)
        .await
        .unwrap();
    for cap in ["crm:read", "crm:manage", "business_master_data:read"] {
        grant(&pool, role, cap, restricted(unit)).await;
    }
    let snapshot = store
        .snapshot_for_permission(user, "crm:read")
        .await
        .unwrap();
    assert!(snapshot.permission_keys.contains("crm:manage"));
    assert_eq!(
        snapshot
            .scopes
            .business_unit_ids
            .into_iter()
            .collect::<Vec<_>>(),
        vec![unit]
    );
    assert_eq!(
        snapshot
            .scopes
            .legal_entity_ids
            .into_iter()
            .collect::<Vec<_>>(),
        vec![legal]
    );
    assert_eq!(snapshot.scopes.customer_ids.len(), 2);
    assert!(!snapshot.permission_keys.contains("sales_order:read"));
    // A capability outside this bridge must not advertise an unsupported edit action.
    grant(&pool, role, "business_master_data:manage", restricted(unit)).await;
    let masters = CoreMasterDataService::new(store.clone());
    let customers = masters
        .list(user, Some(CoreMasterType::Customer), 100)
        .await
        .unwrap();
    assert_eq!(customers.items.len(), 2);
    assert!(!customers.can_manage);
    let units = masters
        .list(user, Some(CoreMasterType::BusinessUnit), 100)
        .await
        .unwrap();
    assert_eq!(units.items.len(), 1);
    assert_eq!(units.items[0].id, unit);
    let crm = CrmService::new(store.clone());
    let owners = crm
        .owners(
            user,
            &OwnerScope {
                legal_entity_id: legal,
                business_unit_id: unit,
                customer_id: Some(customer),
                query: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(owners["items"].as_array().unwrap().len(), 1);
    assert_eq!(owners["items"][0]["id"], user.to_string());
    let options = crm.options(user).await.unwrap();
    let units = options["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|x| x["resourceType"] == "business_unit")
        .collect::<Vec<_>>();
    assert_eq!(units.len(), 1);
    assert_eq!(units[0]["id"], unit.to_string());
    let mut input:SaveOpportunity=serde_json::from_value(json!({
        "legalEntityId":legal,"businessUnitId":unit,"customerId":customer,
        "title":"Scoped presales test","companyName":"Test company","contactName":"Test contact","contactDetails":"",
        "stage":"new","currency":"CNY","nextAction":"Follow up"
    })).unwrap();
    let created = crm
        .save(user, Uuid::new_v4(), None, "iam-create-allowed", &input)
        .await
        .unwrap();
    let opportunity: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    assert!(crm.detail(user, opportunity, 0).await.is_ok());
    for forbidden in [child, outside] {
        input.business_unit_id = forbidden;
        assert!(crm
            .save(
                user,
                Uuid::new_v4(),
                None,
                &format!("iam-denied-{forbidden}"),
                &input
            )
            .await
            .is_err());
    }
    // A wider/different manage grant cannot turn a read grant into write access.
    grant(&pool, role, "crm:manage", restricted(outside)).await;
    let read = store
        .snapshot_for_permission(user, "crm:read")
        .await
        .unwrap();
    assert!(!read.permission_keys.contains("crm:manage"));
    let manage = store
        .snapshot_for_permission(user, "crm:manage")
        .await
        .unwrap();
    assert!(!manage.scopes.business_unit_ids.contains(&unit));
    assert!(manage.scopes.business_unit_ids.contains(&outside));
    input.business_unit_id = outside;
    let foreign = crm
        .save(user, Uuid::new_v4(), None, "iam-create-other", &input)
        .await
        .unwrap();
    let foreign: Uuid = serde_json::from_value(foreign["id"].clone()).unwrap();
    assert!(crm.detail(user, foreign, 0).await.is_err());
    let filters: Filters = serde_json::from_value(json!({})).unwrap();
    let list = crm.list(user, &filters).await.unwrap();
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["canManage"], false);
    // Replacing the scope or adding an obligation takes effect on the next request.
    grant(&pool,role,"crm:read",json!({"mode":"restricted","dimensions":{"business_unit":[unit],"customer":[other_customer]}})).await;
    assert!(crm.detail(user, opportunity, 0).await.is_err());
    grant(&pool, role, "crm:read", restricted(unit)).await;
    sqlx::query("UPDATE business_iam.role_permissions SET obligations='[\"human_approval\"]'::jsonb WHERE role_id=$1").bind(role).execute(&pool).await.unwrap();
    assert!(crm.options(user).await.is_err());
    sqlx::query(
        "UPDATE business_iam.role_permissions SET obligations='[]'::jsonb WHERE role_id=$1",
    )
    .bind(role)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE business_iam.principal_roles SET valid_from=now()-interval '2 days',valid_until=now()-interval '1 second' WHERE principal_id=$1").bind(principal).execute(&pool).await.unwrap();
    assert!(crm.options(user).await.is_err());
    sqlx::query("UPDATE business_iam.principal_roles SET valid_until=NULL WHERE principal_id=$1")
        .bind(principal)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE business_iam.roles SET status='disabled' WHERE id=$1")
        .bind(role)
        .execute(&pool)
        .await
        .unwrap();
    assert!(crm.options(user).await.is_err());
    sqlx::query("UPDATE business_iam.roles SET status='active' WHERE id=$1")
        .bind(role)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE business_iam.principals SET status='disabled',disabled_at=now() WHERE id=$1",
    )
    .bind(principal)
    .execute(&pool)
    .await
    .unwrap();
    assert!(crm.options(user).await.is_err());
    sqlx::query("UPDATE business_iam.principals SET status='active',disabled_at=NULL WHERE id=$1")
        .bind(principal)
        .execute(&pool)
        .await
        .unwrap();
    assert!(crm.options(user).await.is_ok());
    sqlx::query("DELETE FROM business_iam.principal_roles WHERE principal_id=$1")
        .bind(principal)
        .execute(&pool)
        .await
        .unwrap();
    assert!(crm.options(user).await.is_err());
    // Direct human grants use the same exact-node semantics and validity checks.
    sqlx::query("INSERT INTO business_iam.principal_permissions(principal_id,permission_id,data_scope) SELECT $1,id,$2 FROM business_iam.permissions WHERE capability='crm:read'")
        .bind(principal).bind(restricted(unit)).execute(&pool).await.unwrap();
    assert!(crm.options(user).await.is_ok());
    sqlx::query("UPDATE business_iam.principal_permissions SET valid_from=now()-interval '2 days',valid_until=now()-interval '1 second' WHERE principal_id=$1")
        .bind(principal).execute(&pool).await.unwrap();
    assert!(crm.options(user).await.is_err());
}
