//! Real lock-wait regressions for master writes and idempotent replay authority.
use business_core::{
    b2::DomainError,
    master_data::{
        ChangeCoreMasterStatus, CoreMasterDataService, CoreMasterType, SaveCoreMasterData,
    },
    product_master::{
        ChangeProductMasterStatus, ProductMasterService, ProductMasterType, SaveProductMasterData,
    },
    PgStore,
};
use serde_json::{json, Value};
use sqlx::{postgres::PgPoolOptions, AssertSqlSafe, PgPool};
use uuid::Uuid;

async fn waiting(pool: &PgPool, blocker: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(blocker)
            .fetch_one(pool)
            .await
            .unwrap();
            if blocked {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("operation must actually wait on the held lock");
}
async fn invoke(
    pool: &PgPool,
    actor: Uuid,
    product: bool,
    id: Option<Uuid>,
    key: &str,
    input: Value,
    status: bool,
) -> Result<Value, DomainError> {
    if product {
        let service = ProductMasterService::new(PgStore::new(pool.clone()));
        let result = if status {
            service
                .change_status(
                    actor,
                    Uuid::new_v4(),
                    ProductMasterType::Brand,
                    id.unwrap(),
                    key,
                    &serde_json::from_value::<ChangeProductMasterStatus>(input).unwrap(),
                )
                .await?
        } else {
            service
                .save(
                    actor,
                    Uuid::new_v4(),
                    id,
                    key,
                    &serde_json::from_value::<SaveProductMasterData>(input).unwrap(),
                )
                .await?
        };
        Ok(serde_json::to_value(result).unwrap())
    } else {
        let service = CoreMasterDataService::new(PgStore::new(pool.clone()));
        let result = if status {
            service
                .change_status(
                    actor,
                    Uuid::new_v4(),
                    CoreMasterType::Customer,
                    id.unwrap(),
                    key,
                    &serde_json::from_value::<ChangeCoreMasterStatus>(input).unwrap(),
                )
                .await?
        } else {
            service
                .save(
                    actor,
                    Uuid::new_v4(),
                    id,
                    key,
                    &serde_json::from_value::<SaveCoreMasterData>(input).unwrap(),
                )
                .await?
        };
        Ok(serde_json::to_value(result).unwrap())
    }
}
#[tokio::test]
async fn master_writes_recheck_authority_and_version_after_real_waits() {
    let Ok(url) = std::env::var("BUSINESS_CORE_MASTER_AUTHORITY_TEST_DATABASE_URL") else {
        eprintln!("skipping: BUSINESS_CORE_MASTER_AUTHORITY_TEST_DATABASE_URL unset");
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .unwrap();
    PgStore::new(pool.clone()).migrate().await.unwrap();
    let actor = Uuid::new_v4();
    let role = Uuid::new_v4();
    let legal = Uuid::new_v4();
    let unit = Uuid::new_v4();
    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'master-authority-test',$1::text,'Master tester')").bind(actor).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_roles(id,role_key,name) VALUES($1,'master_authority_test','Master tester')").bind(role).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,'business_master_data:manage'),($1,'business_product_master:manage')").bind(role).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) VALUES($1,$2,$1)",
    )
    .bind(actor)
    .bind(role)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_legal_entities(id,code,name,country_code,functional_currency) VALUES($1,'MASTER_LE','Master LE','CN','CNY')").bind(legal).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_units(id,legal_entity_id,code,name) VALUES($1,$2,'MASTER_BU','Master BU')").bind(unit).bind(legal).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_legal_entity_scopes(enterprise_user_id,legal_entity_id,granted_by) VALUES($1,$2,$1)").bind(actor).bind(legal).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)").bind(actor).bind(unit).execute(&pool).await.unwrap();
    for product in [false, true] {
        let input = if product {
            json!({"resourceType":"brand","code":"MASTER_BRAND","name":"Original brand"})
        } else {
            json!({"resourceType":"customer","code":"MASTER_CUSTOMER","name":"Original customer","legalEntityId":legal,"businessUnitId":unit,"creditCurrency":"CNY"})
        };
        let create_key = format!("master-create-{product}");
        let first = invoke(
            &pool,
            actor,
            product,
            None,
            &create_key,
            input.clone(),
            false,
        )
        .await
        .unwrap();
        let id: Uuid = serde_json::from_value(first["id"].clone()).unwrap();
        assert_eq!(first["version"], 1);
        let table = if product {
            "business_brands"
        } else {
            "business_customers"
        };
        let scope_table = if product {
            "business_brand_scopes"
        } else {
            "business_customer_scopes"
        };
        let column = if product { "brand_id" } else { "customer_id" };
        let revoke =
            format!("DELETE FROM {scope_table} WHERE enterprise_user_id=$1 AND {column}=$2");
        let restore = format!(
            "INSERT INTO {scope_table}(enterprise_user_id,{column},granted_by) VALUES($1,$2,$1)"
        );
        let state_sql=format!("SELECT jsonb_build_object('name',name,'status',status,'version',version) FROM {table} WHERE id=$1");
        let before: Value = sqlx::query_scalar(AssertSqlSafe(state_sql.clone()))
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
        let permission = if product {
            "business_product_master:manage"
        } else {
            "business_master_data:manage"
        };
        for (status, permission_revoke) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let mut lock = pool.begin().await.unwrap();
            sqlx::query(AssertSqlSafe(format!(
                "SELECT id FROM {table} WHERE id=$1 FOR UPDATE"
            )))
            .bind(id)
            .fetch_one(&mut *lock)
            .await
            .unwrap();
            let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *lock)
                .await
                .unwrap();
            let mut command = if status {
                json!({"status":"disabled","expectedVersion":1})
            } else {
                input.clone()
            };
            if !status {
                command["expectedVersion"] = json!(1);
                command["name"] = json!("must not commit");
            }
            let p = pool.clone();
            let task = tokio::spawn(async move {
                invoke(
                    &p,
                    actor,
                    product,
                    Some(id),
                    &format!("master-wait-{product}-{status}-{permission_revoke}"),
                    command,
                    status,
                )
                .await
            });
            waiting(&pool, blocker).await;
            if permission_revoke {
                sqlx::query(
                    "DELETE FROM business_role_permissions WHERE role_id=$1 AND permission_key=$2",
                )
                .bind(role)
                .bind(permission)
                .execute(&pool)
                .await
                .unwrap();
            } else {
                sqlx::query(AssertSqlSafe(revoke.clone()))
                    .bind(actor)
                    .bind(id)
                    .execute(&pool)
                    .await
                    .unwrap();
            }
            lock.commit().await.unwrap();
            assert!(matches!(
                task.await.unwrap(),
                Err(DomainError::NotFoundOrForbidden)
            ));
            let after: Value = sqlx::query_scalar(AssertSqlSafe(state_sql.clone()))
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(
                before, after,
                "denied write must leave business fields/version unchanged"
            );
            // Original creation replay must not disclose an inaccessible object or re-grant scope.
            assert!(matches!(
                invoke(
                    &pool,
                    actor,
                    product,
                    None,
                    &create_key,
                    input.clone(),
                    false
                )
                .await,
                Err(DomainError::NotFoundOrForbidden)
            ));
            if permission_revoke {
                sqlx::query(
                    "INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,$2)",
                )
                .bind(role)
                .bind(permission)
                .execute(&pool)
                .await
                .unwrap();
            } else {
                sqlx::query(AssertSqlSafe(restore.clone()))
                    .bind(actor)
                    .bind(id)
                    .execute(&pool)
                    .await
                    .unwrap();
            }
        }
        // A replay also waits on the current object, then rechecks scope.
        let mut lock = pool.begin().await.unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "SELECT id FROM {table} WHERE id=$1 FOR UPDATE"
        )))
        .bind(id)
        .fetch_one(&mut *lock)
        .await
        .unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        let p = pool.clone();
        let replay_input = input.clone();
        let replay_key = create_key.clone();
        let task = tokio::spawn(async move {
            invoke(&p, actor, product, None, &replay_key, replay_input, false).await
        });
        waiting(&pool, blocker).await;
        sqlx::query(AssertSqlSafe(revoke.clone()))
            .bind(actor)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        lock.commit().await.unwrap();
        assert!(matches!(
            task.await.unwrap(),
            Err(DomainError::NotFoundOrForbidden)
        ));
        sqlx::query(AssertSqlSafe(restore.clone()))
            .bind(actor)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        let replay = invoke(
            &pool,
            actor,
            product,
            None,
            &create_key,
            input.clone(),
            false,
        )
        .await
        .unwrap();
        assert_eq!(replay["id"], first["id"]);
        assert_eq!(replay["idempotentReplay"], true);
        // A concurrent legitimate update wins. The waiter must observe its new version.
        let mut lock = pool.begin().await.unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "UPDATE {table} SET name='Concurrent update',version=2 WHERE id=$1"
        )))
        .bind(id)
        .execute(&mut *lock)
        .await
        .unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        let mut command = input.clone();
        command["expectedVersion"] = json!(1);
        command["name"] = json!("stale overwrite");
        let p = pool.clone();
        let task = tokio::spawn(async move {
            invoke(
                &p,
                actor,
                product,
                Some(id),
                &format!("stale-version-{product}"),
                command,
                false,
            )
            .await
        });
        waiting(&pool, blocker).await;
        lock.commit().await.unwrap();
        assert!(matches!(
            task.await.unwrap(),
            Err(DomainError::VersionConflict)
        ));
        let after: Value = sqlx::query_scalar(AssertSqlSafe(state_sql))
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(after["name"], "Concurrent update");
        let mut good = input.clone();
        good["expectedVersion"] = after["version"].clone();
        good["name"] = json!("Authorized update");
        let updated = invoke(
            &pool,
            actor,
            product,
            Some(id),
            &format!("good-update-{product}"),
            good,
            false,
        )
        .await
        .unwrap();
        let status = json!({"status":"disabled","expectedVersion":updated["version"]});
        let changed = invoke(
            &pool,
            actor,
            product,
            Some(id),
            &format!("good-status-{product}"),
            status.clone(),
            true,
        )
        .await
        .unwrap();
        assert_eq!(changed["status"], "disabled");
        let replay = invoke(
            &pool,
            actor,
            product,
            Some(id),
            &format!("good-status-{product}"),
            status,
            true,
        )
        .await
        .unwrap();
        assert_eq!(replay["version"], changed["version"]);
        assert_eq!(replay["idempotentReplay"], true);
    }
    // Customer insertion waits on its real parent FK lock. Revoking the parent
    // scope while waiting must not be undone by grant_creator_scope afterward.
    let mut lock = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM business_legal_entities WHERE id=$1 FOR UPDATE")
        .bind(legal)
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let p = pool.clone();
    let task = tokio::spawn(async move {
        invoke(&p,actor,false,None,"revoked-creation",json!({"resourceType":"customer","code":"DENIED_CUSTOMER","name":"Denied","legalEntityId":legal,"businessUnitId":unit,"creditCurrency":"CNY"}),false).await
    });
    waiting(&pool, blocker).await;
    sqlx::query("DELETE FROM business_legal_entity_scopes WHERE enterprise_user_id=$1 AND legal_entity_id=$2").bind(actor).bind(legal).execute(&pool).await.unwrap();
    lock.commit().await.unwrap();
    assert!(matches!(
        task.await.unwrap(),
        Err(DomainError::NotFoundOrForbidden)
    ));
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM business_customers WHERE code='DENIED_CUSTOMER'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    let scopes:i64=sqlx::query_scalar("SELECT count(*) FROM business_legal_entity_scopes WHERE enterprise_user_id=$1 AND legal_entity_id=$2").bind(actor).bind(legal).fetch_one(&pool).await.unwrap();
    assert_eq!(scopes, 0);
    // A brand creation can wait on the authorization revision itself. The
    // permission revoked by that lock holder must be re-read before creator grants.
    let mut lock = pool.begin().await.unwrap();
    sqlx::query("SELECT revision FROM business_authorization_revision WHERE singleton FOR UPDATE")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let p = pool.clone();
    let task = tokio::spawn(async move {
        invoke(
            &p,
            actor,
            true,
            None,
            "denied-brand-create",
            json!({"resourceType":"brand","code":"DENIED_BRAND","name":"Denied brand"}),
            false,
        )
        .await
    });
    waiting(&pool, blocker).await;
    sqlx::query("DELETE FROM business_role_permissions WHERE role_id=$1 AND permission_key='business_product_master:manage'").bind(role).execute(&mut *lock).await.unwrap();
    lock.commit().await.unwrap();
    assert!(matches!(
        task.await.unwrap(),
        Err(DomainError::NotFoundOrForbidden)
    ));
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM business_brands WHERE code='DENIED_BRAND'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,'business_product_master:manage')").bind(role).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_legal_entity_scopes(enterprise_user_id,legal_entity_id,granted_by) VALUES($1,$2,$1)").bind(actor).bind(legal).execute(&pool).await.unwrap();
    // Two successful creates both grant scope; neither may deadlock upgrading a shared revision lock.
    let create_one = invoke(
        &pool,
        actor,
        false,
        None,
        "parallel-create-one",
        json!({"resourceType":"customer","code":"PARALLEL_ONE","name":"One","legalEntityId":legal,"businessUnitId":unit,"creditCurrency":"CNY"}),
        false,
    );
    let create_two = invoke(
        &pool,
        actor,
        false,
        None,
        "parallel-create-two",
        json!({"resourceType":"customer","code":"PARALLEL_TWO","name":"Two","legalEntityId":legal,"businessUnitId":unit,"creditCurrency":"CNY"}),
        false,
    );
    let (one, two) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(create_one, create_two)
    })
    .await
    .unwrap();
    assert_eq!(one.unwrap()["version"], 1);
    assert_eq!(two.unwrap()["version"], 1);
    // Every Core master family retains only its new-object creator scope.
    let new_legal=invoke(&pool,actor,false,None,"new-legal",json!({"resourceType":"legal_entity","code":"NEW_LEGAL","name":"New legal","countryCode":"CN","functionalCurrency":"CNY"}),false).await.unwrap();
    let new_unit=invoke(&pool,actor,false,None,"new-unit",json!({"resourceType":"business_unit","code":"NEW_UNIT","name":"New unit","legalEntityId":new_legal["id"]}),false).await.unwrap();
    let supplier=invoke(&pool,actor,false,None,"new-supplier",json!({"resourceType":"supplier","code":"NEW_SUPPLIER","name":"New supplier","legalEntityId":new_legal["id"],"businessUnitId":new_unit["id"]}),false).await.unwrap();
    let warehouse=invoke(&pool,actor,false,None,"new-warehouse",json!({"resourceType":"warehouse","code":"NEW_WAREHOUSE","name":"New warehouse","legalEntityId":new_legal["id"],"businessUnitId":new_unit["id"]}),false).await.unwrap();
    for (result, table, column) in [
        (new_legal, "business_legal_entity_scopes", "legal_entity_id"),
        (new_unit, "business_unit_scopes", "business_unit_id"),
        (supplier, "business_supplier_scopes", "supplier_id"),
        (warehouse, "business_warehouse_scopes", "warehouse_id"),
    ] {
        let id: Uuid = serde_json::from_value(result["id"].clone()).unwrap();
        let count: i64 = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT count(*) FROM {table} WHERE enterprise_user_id=$1 AND {column}=$2"
        )))
        .bind(actor)
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
    }
    let audits:i64=sqlx::query_scalar("SELECT count(*) FROM business_core_audit_events WHERE operation IN ('CORE_MASTER_DATA_SAVED','CORE_MASTER_DATA_STATUS_CHANGED','PRODUCT_MASTER_DATA_SAVED','PRODUCT_MASTER_DATA_STATUS_CHANGED')").fetch_one(&pool).await.unwrap();
    assert_eq!(
        audits, 12,
        "only authorized creates, updates and status changes may be audited"
    );
}
