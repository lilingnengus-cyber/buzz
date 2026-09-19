#[path = "support/master_guarded_saves.rs"]
mod master_guarded_saves;
use business_core::{
    b2::DomainError,
    master_data::{CoreMasterCommand, CoreMasterDataService, SaveCoreMasterData},
    product_master::{ProductMasterCommand, ProductMasterService, SaveProductMasterData},
    PgStore,
};
use serde_json::{json, Value};
use sqlx::{postgres::PgPoolOptions, PgPool};
use uuid::Uuid;
async fn preview(
    pool: &PgPool,
    actor: Uuid,
    product: bool,
    input: Value,
) -> Result<Value, DomainError> {
    if product {
        ProductMasterService::new(PgStore::new(pool.clone()))
            .command_preview(
                actor,
                &serde_json::from_value::<ProductMasterCommand>(input).unwrap(),
            )
            .await
    } else {
        CoreMasterDataService::new(PgStore::new(pool.clone()))
            .command_preview(
                actor,
                &serde_json::from_value::<CoreMasterCommand>(input).unwrap(),
            )
            .await
    }
}
async fn create(pool: &PgPool, actor: Uuid, product: bool, input: &Value) -> Uuid {
    if product {
        ProductMasterService::new(PgStore::new(pool.clone()))
            .save(
                actor,
                Uuid::new_v4(),
                None,
                &Uuid::new_v4().to_string(),
                &serde_json::from_value::<SaveProductMasterData>(input.clone()).unwrap(),
            )
            .await
            .unwrap()
            .id
    } else {
        CoreMasterDataService::new(PgStore::new(pool.clone()))
            .save(
                actor,
                Uuid::new_v4(),
                None,
                &Uuid::new_v4().to_string(),
                &serde_json::from_value::<SaveCoreMasterData>(input.clone()).unwrap(),
            )
            .await
            .unwrap()
            .id
    }
}
#[tokio::test]
async fn all_master_families_have_stable_read_only_previews() {
    let Ok(url) = std::env::var("BUSINESS_CORE_MASTER_PREVIEW_TEST_DATABASE_URL") else {
        eprintln!("skipping: BUSINESS_CORE_MASTER_PREVIEW_TEST_DATABASE_URL unset");
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .unwrap();
    PgStore::new(pool.clone()).migrate().await.unwrap();
    let actor = Uuid::new_v4();
    let outsider = Uuid::new_v4();
    let role = Uuid::new_v4();
    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'master-preview',$1::text,'Master preview'),($2,'master-preview',$2::text,'Outside preview')").bind(actor).bind(outsider).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_roles(id,role_key,name) VALUES($1,'master_preview','Master preview')",
    )
    .bind(role)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,'business_master_data:manage'),($1,'business_product_master:manage')").bind(role).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) VALUES($1,$2,$1),($3,$2,$1)").bind(actor).bind(role).bind(outsider).execute(&pool).await.unwrap();
    let mut entries = Vec::new();
    let input = json!({"resourceType":"legal_entity","code":"PV_LE","name":"Preview entity","countryCode":"CN","functionalCurrency":"CNY"});
    let legal = create(&pool, actor, false, &input).await;
    entries.push((false, legal, input));
    let input = json!({"resourceType":"business_unit","code":"PV_BU","name":"Preview unit","legalEntityId":legal});
    let unit = create(&pool, actor, false, &input).await;
    entries.push((false, unit, input));
    for kind in ["customer", "supplier", "warehouse"] {
        let mut input = json!({"resourceType":kind,"code":format!("PV_{}",kind.to_uppercase()),"name":format!("Preview {kind}"),"legalEntityId":legal,"businessUnitId":unit});
        if kind == "customer" {
            input["creditCurrency"] = json!("CNY");
            input["creditLimitMinor"] = json!(78900);
        }
        let id = create(&pool, actor, false, &input).await;
        entries.push((false, id, input));
    }
    let input =
        json!({"resourceType":"unit_of_measure","code":"PV_EA","name":"Each","precisionScale":0});
    let uom = create(&pool, actor, true, &input).await;
    entries.push((true, uom, input));
    let alternate = create(
        &pool,
        actor,
        true,
        &json!({"resourceType":"unit_of_measure","code":"PV_BOX","name":"Box","precisionScale":0}),
    )
    .await;
    let input = json!({"resourceType":"product_category","code":"PV_CATEGORY","name":"Category"});
    let category = create(&pool, actor, true, &input).await;
    entries.push((true, category, input));
    let input = json!({"resourceType":"brand","code":"PV_BRAND","name":"Brand"});
    let brand = create(&pool, actor, true, &input).await;
    entries.push((true, brand, input));
    let input = json!({"resourceType":"product","code":"PV_PRODUCT","name":"Product","categoryId":category,"brandId":brand,"baseUomId":uom});
    let product = create(&pool, actor, true, &input).await;
    entries.push((true, product, input));
    let input = json!({"resourceType":"sku","code":"PV_SKU","name":"SKU","productId":product,"barcode":"PV001"});
    let sku = create(&pool, actor, true, &input).await;
    entries.push((true, sku, input));
    let input = json!({"resourceType":"uom_conversion","code":"","name":"","productId":product,"unitOfMeasureId":alternate,"factorToBase":"12.00000000","usageScope":"both"});
    let conversion = create(&pool, actor, true, &input).await;
    entries.push((true, conversion, input));
    assert_eq!(entries.len(), 11);
    let preview_unit=create(&pool,actor,true,&json!({"resourceType":"unit_of_measure","code":"PV_PREVIEW_UNIT","name":"Preview only unit","precisionScale":0})).await;

    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM business_core_audit_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    for (is_product, id, input) in &entries {
        let mut fresh = input.clone();
        if input["resourceType"] == "uom_conversion" {
            fresh["unitOfMeasureId"] = json!(preview_unit);
        } else {
            fresh["code"] = json!(format!("{}_NEXT", input["code"].as_str().unwrap()));
        }
        let creation = json!({"operation":"create","command":fresh});
        assert!(matches!(
            preview(
                &pool,
                actor,
                *is_product,
                json!({"operation":"create","command":input})
            )
            .await,
            Err(DomainError::Invalid(_))
        ));
        let made = preview(&pool, actor, *is_product, creation).await.unwrap();
        assert_eq!(made["documentId"], Value::Null);
        assert_eq!(made["current"], Value::Null);
        let mut update = input.clone();
        update["expectedVersion"] = json!(1);
        if update["resourceType"] != "uom_conversion" {
            update["name"] = json!("  Updated name  ");
        }
        let command = json!({"operation":"update","documentId":id,"command":update});
        let first = preview(&pool, actor, *is_product, command.clone())
            .await
            .unwrap();
        assert_eq!(
            first,
            preview(&pool, actor, *is_product, command.clone())
                .await
                .unwrap()
        );
        assert_eq!(first["current"]["version"], 1);
        if input["resourceType"] != "uom_conversion" {
            assert_eq!(first["effectiveFields"]["name"], "Updated name");
        }
        if input["resourceType"] == "customer" {
            assert_eq!(first["effectiveFields"]["creditLimitMinor"], 78900);
            assert_eq!(first["effectiveFields"]["paymentTermsDays"], 30);
        }
        let mut bad = command.clone();
        bad["command"]["expectedVersion"] = json!(2);
        assert!(matches!(
            preview(&pool, actor, *is_product, bad).await,
            Err(DomainError::VersionConflict)
        ));
        if input["resourceType"] != "uom_conversion" {
            let mut bad = command.clone();
            bad["command"]["code"] = json!("CHANGED_CODE");
            assert!(matches!(
                preview(&pool, actor, *is_product, bad).await,
                Err(DomainError::Invalid(_))
            ));
        }
        let status = json!({"operation":"change_status","resourceType":input["resourceType"],"documentId":id,"command":{"status":"disabled","expectedVersion":1}});
        let status = preview(&pool, actor, *is_product, status).await.unwrap();
        let blocked = matches!(
            input["resourceType"].as_str().unwrap(),
            "legal_entity"
                | "business_unit"
                | "unit_of_measure"
                | "product_category"
                | "brand"
                | "product"
        );
        assert_eq!(status["canExecute"], !blocked, "{}", input["resourceType"]);
        assert!(!status["disableImpacts"].as_array().unwrap().is_empty());
        if !*is_product
            || matches!(
                input["resourceType"].as_str().unwrap(),
                "brand" | "product" | "sku" | "uom_conversion"
            )
        {
            assert!(matches!(
                preview(&pool, outsider, *is_product, command).await,
                Err(DomainError::NotFoundOrForbidden)
            ));
        }
    }
    http_preview_checks(&pool, actor, outsider, legal, unit, product, preview_unit).await;
    // Parent changes bind the full parent state even without changing the child's version.
    let command = json!({"operation":"update","documentId":sku,"command":{"resourceType":"sku","code":"PV_SKU","name":"SKU","productId":product,"expectedVersion":1}});
    let original = preview(&pool, actor, true, command.clone()).await.unwrap();
    sqlx::query("UPDATE business_brands SET name=name WHERE id=$1")
        .bind(brand)
        .execute(&pool)
        .await
        .unwrap();
    let metadata_changed = preview(&pool, actor, true, command.clone()).await.unwrap();
    assert_eq!(original["current"], metadata_changed["current"]);
    assert_ne!(original["parents"], metadata_changed["parents"]);
    sqlx::query("UPDATE business_brands SET name='Renamed parent' WHERE id=$1")
        .bind(brand)
        .execute(&pool)
        .await
        .unwrap();
    let changed = preview(&pool, actor, true, command.clone()).await.unwrap();
    assert_ne!(original, changed);
    assert_eq!(changed["current"]["version"], 1);
    assert_eq!(changed["parents"]["brand"]["name"], "Renamed parent");
    // Fields ignored by the legacy write API cannot become misleading approved commands.
    let invalid = json!({"operation":"create","command":{"resourceType":"brand","code":"BAD_FIELDS","name":"Bad","barcode":"ignored"}});
    assert!(matches!(
        preview(&pool, actor, true, invalid).await,
        Err(DomainError::Invalid(_))
    ));
    let mut lock = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM business_products WHERE id=$1 FOR UPDATE")
        .bind(product)
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let p = pool.clone();
    let task = tokio::spawn(async move { preview(&p, actor, true, command).await });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(blocker)
            .fetch_one(&pool)
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    sqlx::query("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2")
        .bind(actor)
        .bind(brand)
        .execute(&pool)
        .await
        .unwrap();
    lock.commit().await.unwrap();
    assert!(matches!(
        task.await.unwrap(),
        Err(DomainError::NotFoundOrForbidden)
    ));
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM business_core_audit_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(before, after);
    let changed_versions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM core_master_data_maintenance WHERE version<>1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(changed_versions, 0);
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)")
        .bind(actor).bind(brand).execute(&pool).await.unwrap();
    master_guarded_saves::check(&pool, actor, &entries, preview_unit).await;
}

async fn http_preview_checks(
    pool: &PgPool,
    actor: Uuid,
    outsider: Uuid,
    legal: Uuid,
    unit: Uuid,
    product: Uuid,
    conversion_unit: Uuid,
) {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;
    let config = business_core::Config::from_env()
        .expect("preview HTTP tests require isolated Core service configuration");
    let app = business_core::router(business_core::AppState::new(
        PgStore::new(pool.clone()),
        &config,
    ));
    let credential = std::env::var("BUSINESS_CORE_SERVICE_CREDENTIAL").unwrap();
    let input = json!({"operation":"create","command":{"resourceType":"customer","code":"HTTP_CUSTOMER","name":"HTTP preview","legalEntityId":legal,"businessUnitId":unit,"creditCurrency":"CNY"}});
    for (authorized, caller, payload, expected) in [
        (true, actor, input.clone(), 200),
        (false, actor, input.clone(), 401),
        (true, outsider, input.clone(), 404),
        (
            true,
            actor,
            json!({"operation":"create","command":input["command"],"sql":"not permitted"}),
            422,
        ),
    ] {
        let trace = Uuid::new_v4();
        let mut request = Request::builder()
            .method("POST")
            .uri("/v1/agent-core-master-previews")
            .header("x-service-audience", "business-core")
            .header("x-enterprise-user-id", caller.to_string())
            .header("x-trace-id", trace.to_string())
            .header("content-type", "application/json");
        if authorized {
            request = request.header("x-business-service-credential", &credential);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(payload.to_string())).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), expected);
        if expected == 200 {
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 1000000).await.unwrap())
                    .unwrap();
            assert_eq!(body["traceId"], trace.to_string());
            assert_eq!(
                body["document"]["documentType"],
                "core_master_creation_intent"
            );
            assert_eq!(body["document"]["effectiveFields"]["creditLimitMinor"], 0);
        }
    }
    let request=Request::builder().method("POST").uri("/v1/agent-product-master-previews")
        .header("x-service-audience","business-core").header("x-enterprise-user-id",actor.to_string()).header("x-trace-id",Uuid::new_v4().to_string())
        .header("x-business-service-credential",&credential).header("content-type","application/json")
        .body(Body::from(json!({"operation":"create","command":{"resourceType":"uom_conversion","code":"","name":"","productId":product,"unitOfMeasureId":conversion_unit,"factorToBase":"0.33333333","usageScope":"both"}}).to_string())).unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1000000).await.unwrap()).unwrap();
    assert_eq!(
        body["document"]["effectiveFields"]["factorToBase"],
        "0.33333333"
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM business_customers WHERE code='HTTP_CUSTOMER'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
}
