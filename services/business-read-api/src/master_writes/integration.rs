use super::*;
#[path = "browser_test.rs"]
mod browser_test;
#[path = "lookup_test.rs"]
mod lookup_test;
// Optional isolated-test response corpus for the downstream MCP validator.
fn export(tool: &str, trace: Uuid, result: &Value) {
    use std::io::Write;
    if let Ok(path) = std::env::var("BUSINESS_MASTER_MCP_FIXTURE_FILE") {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(
            file,
            "{}",
            json!({"tool":tool,"traceId":trace,"result":result})
        )
        .unwrap();
    }
}
async fn create(
    core: &CoreClient,
    actor: Uuid,
    fields: Value,
    dimensions: &[(&str, Uuid)],
) -> Uuid {
    let family = input::family_of_resource(fields["resourceType"].as_str().unwrap()).unwrap();
    let tool = format!("prepare_{family}_master_creation");
    let c = context(actor, &tool);
    let authorized = grant(&c, dimensions);
    let prepared = value(forward(core, &tool, fields.clone(), &c, &authorized).await).await;
    export(&tool, c.trace_id, &prepared);
    assert!(prepared["document"]["current"].is_null());
    if fields["resourceType"] == "unit_of_measure" {
        assert_eq!(prepared["document"]["effectiveFields"]["precisionScale"], 0);
    }
    let replay = value(forward(core, &tool, fields, &c, &authorized).await).await;
    assert_eq!(prepared["item"]["id"], replay["item"]["id"]);
    let tool = format!("approve_{family}_master_creation");
    let c = context(actor, &tool);
    let authorized = grant(&c, dimensions);
    let command = json!({"documentId":prepared["item"]["id"],"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve"});
    let result = value(forward(core, &tool, command, &c, &authorized).await).await;
    export(&tool, c.trace_id, &result);
    assert_eq!(result["executed"], true);
    assert_eq!(result["createdDocument"]["version"], 1);
    assert_eq!(result["createdDocument"]["traceId"], c.trace_id.to_string());
    Uuid::parse_str(result["createdDocument"]["id"].as_str().unwrap()).unwrap()
}
async fn intents(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM business_agent_master_intents")
        .fetch_one(pool)
        .await
        .unwrap()
}
#[tokio::test]
async fn real_core_master_adapter_preserves_fields_and_enforces_intersections() {
    let Ok(url) = std::env::var("BUSINESS_MASTER_ADAPTER_TEST_DATABASE_URL") else {
        eprintln!("skipping: BUSINESS_MASTER_ADAPTER_TEST_DATABASE_URL unset");
        return;
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(12)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let actor = Uuid::new_v4();
    let role = Uuid::new_v4();
    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'master-adapter',$1::text,'Master user')").bind(actor).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_roles(id,role_key,name) VALUES($1,'master_adapter','Master adapter')",
    )
    .bind(role)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) VALUES($1,$2,$1)",
    )
    .bind(actor)
    .bind(role)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,'business_master_data:manage'),($1,'business_product_master:manage'),($1,'business_master_data:read')").bind(role).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES('business_master_data:manage','business_master_data:manage',ARRAY['master_adapter'],1,true),('business_product_master:manage','business_product_master:manage',ARRAY['master_adapter'],1,true)").execute(&pool).await.unwrap();
    let config = business_core::Config::from_env().unwrap();
    let browser_cookie = config.business_session_cookie_name.clone();
    let router = business_core::router(business_core::AppState::new(store, &config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let core = CoreClient {
        client: reqwest::Client::new(),
        base_url: Url::parse(&format!("http://{address}/")).unwrap(),
        credential: config.service_credential,
    };
    let legal=create(&core,actor,json!({"resourceType":"legal_entity","code":"MA_LE","name":"Legal","countryCode":"CN","functionalCurrency":"CNY","registrationNumber":"KEEP_REG"}),&[]).await;
    let unit = create(
        &core,
        actor,
        json!({"resourceType":"business_unit","code":"MA_BU","name":"Unit","legalEntityId":legal}),
        &[("legal_entity", legal)],
    )
    .await;
    let parents = vec![("legal_entity", legal), ("business_unit", unit)];
    let mut entries = vec![
        ("legal_entity", legal, vec![("legal_entity", legal)]),
        ("business_unit", unit, parents.clone()),
    ];
    for kind in ["customer", "supplier", "warehouse"] {
        let mut fields = json!({"resourceType":kind,"code":format!("MA_{kind}").to_uppercase(),"name":kind,"legalEntityId":legal,"businessUnitId":unit});
        if kind == "customer" {
            fields["creditCurrency"] = json!("CNY");
            fields["creditLimitMinor"] = json!(34567);
            fields["paymentTermsDays"] = json!(45);
        }
        if kind == "supplier" {
            fields["paymentTermsDays"] = json!(60);
        }
        if kind == "warehouse" {
            fields["address"] = json!("KEEP_ADDRESS");
        }
        let c = context(actor, "prepare_core_master_creation");
        let denied = grant(&c, &[(kind, Uuid::new_v4())]);
        let before = intents(&pool).await;
        assert_eq!(
            forward(
                &core,
                "prepare_core_master_creation",
                fields.clone(),
                &c,
                &denied
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(intents(&pool).await, before);
        let id = create(&core, actor, fields, &parents).await;
        let mut dimensions = parents.clone();
        dimensions.push((kind, id));
        entries.push((kind, id, dimensions));
    }
    let mut resources = Vec::new();
    for (kind, code) in [
        ("unit_of_measure", "MA_EA"),
        ("unit_of_measure", "MA_BOX"),
        ("product_category", "MA_CATEGORY"),
        ("brand", "MA_BRAND"),
    ] {
        let mut fields = json!({"resourceType":kind,"code":code,"name":kind});
        if kind == "unit_of_measure" {
            fields["precisionScale"] = json!(0);
        }
        let c = context(actor, "prepare_product_master_creation");
        let denied = grant(&c, &[("legal_entity", legal)]);
        let before = intents(&pool).await;
        assert_eq!(
            forward(
                &core,
                "prepare_product_master_creation",
                fields.clone(),
                &c,
                &denied
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(intents(&pool).await, before);
        let id = create(&core, actor, fields, &[]).await;
        resources.push(id);
        if code != "MA_BOX" {
            entries.push((
                kind,
                id,
                if kind == "brand" {
                    vec![("brand", id)]
                } else {
                    vec![]
                },
            ));
        }
    }
    let brand = resources[3];
    let product=create(&core,actor,json!({"resourceType":"product","code":"MA_PRODUCT","name":"Product","categoryId":resources[2],"baseUomId":resources[0],"brandId":brand,"allowZeroCost":true}),&[("brand",brand)]).await;
    entries.push(("product", product, vec![("brand", brand)]));
    let sku=create(&core,actor,json!({"resourceType":"sku","code":"MA_SKU","name":"SKU","productId":product,"barcode":"KEEP_BARCODE"}),&[("brand",brand)]).await;
    entries.push(("sku", sku, vec![("brand", brand)]));
    let conversion=create(&core,actor,json!({"resourceType":"uom_conversion","code":"","name":"","productId":product,"unitOfMeasureId":resources[1],"factorToBase":"12","usageScope":"both"}),&[("brand",brand)]).await;
    entries.push(("uom_conversion", conversion, vec![("brand", brand)]));
    assert_eq!(entries.len(), 11);
    for (kind, id, dimensions) in &entries {
        let family = input::family_of_resource(kind).unwrap();
        let tool = format!("prepare_{family}_master_update");
        let c = context(actor, &tool);
        let authorized = grant(&c, dimensions);
        let changes = if *kind == "uom_conversion" {
            json!({"factorToBase":"2.5"})
        } else {
            json!({"name":"Name only edit"})
        };
        let patch =
            json!({"resourceType":kind,"documentId":id,"expectedVersion":1,"changes":changes});
        let mut bad = patch.clone();
        bad["expectedVersion"] = json!(2);
        let before = intents(&pool).await;
        assert_eq!(
            forward(&core, &tool, bad, &c, &authorized).await.status(),
            StatusCode::CONFLICT
        );
        assert_eq!(intents(&pool).await, before);
        if !dimensions.is_empty() {
            let denied = grant(&c, &[(dimensions.last().unwrap().0, Uuid::new_v4())]);
            assert_eq!(
                forward(&core, &tool, patch.clone(), &c, &denied)
                    .await
                    .status(),
                StatusCode::FORBIDDEN
            );
            assert_eq!(intents(&pool).await, before);
        }
        let prepared = value(forward(&core, &tool, patch, &c, &authorized).await).await;
        export(&tool, c.trace_id, &prepared);
        let fields = &prepared["document"]["command"]["command"];
        match *kind {
            "legal_entity" => assert_eq!(fields["registrationNumber"], "KEEP_REG"),
            "customer" => {
                assert_eq!(fields["creditLimitMinor"], 34567);
                assert_eq!(fields["paymentTermsDays"], 45);
            }
            "supplier" => assert_eq!(fields["paymentTermsDays"], 60),
            "warehouse" => assert_eq!(fields["address"], "KEEP_ADDRESS"),
            "sku" => assert_eq!(fields["barcode"], "KEEP_BARCODE"),
            "product" => assert_eq!(fields["allowZeroCost"], true),
            "uom_conversion" => assert_eq!(fields["usageScope"], "both"),
            _ => (),
        }
        let tool = format!("approve_{family}_master_update");
        let c = context(actor, &tool);
        let authorized = grant(&c, dimensions);
        let command = json!({"documentId":prepared["item"]["id"],"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve"});
        if !dimensions.is_empty() {
            let denied = grant(&c, &[(dimensions.last().unwrap().0, Uuid::new_v4())]);
            assert_eq!(
                forward(&core, &tool, command.clone(), &c, &denied)
                    .await
                    .status(),
                StatusCode::FORBIDDEN
            );
        }
        let result = value(forward(&core, &tool, command, &c, &authorized).await).await;
        export(&tool, c.trace_id, &result);
        assert_eq!(result["createdDocument"]["id"], id.to_string());
        assert_eq!(result["createdDocument"]["version"], 2);
    }
    // Explicit null clears only the named optional field; other fields survive.
    let c = context(actor, "prepare_product_master_update");
    let authorized = grant(&c, &[("brand", brand)]);
    let prepared=value(forward(&core,"prepare_product_master_update",json!({"resourceType":"sku","documentId":sku,"expectedVersion":2,"changes":{"barcode":null}}),&c,&authorized).await).await;
    assert!(prepared["document"]["command"]["command"]["barcode"].is_null());
    assert_eq!(
        prepared["document"]["command"]["command"]["name"],
        "Name only edit"
    );
    sqlx::query("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2")
        .bind(actor)
        .bind(brand)
        .execute(&pool)
        .await
        .unwrap();
    let c = context(actor, "approve_product_master_update");
    let authorized = grant(&c, &[("brand", brand)]);
    assert_eq!(forward(&core,"approve_product_master_update",json!({"documentId":prepared["item"]["id"],"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve"}),&c,&authorized).await.status(),StatusCode::NOT_FOUND);
    let barcode: String = sqlx::query_scalar("SELECT barcode FROM business_skus WHERE id=$1")
        .bind(sku)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(barcode, "KEEP_BARCODE");
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)").bind(actor).bind(brand).execute(&pool).await.unwrap();
    let result = value(forward(&core,"approve_product_master_update",json!({"documentId":prepared["item"]["id"],"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve"}),&c,&authorized).await).await;
    assert_eq!(result["createdDocument"]["version"], 3);
    let barcode: Option<String> =
        sqlx::query_scalar("SELECT barcode FROM business_skus WHERE id=$1")
            .bind(sku)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(barcode.is_none());
    let c = context(actor, "get_business_master_record");
    let input = json!({"resourceType":"sku","documentId":sku});
    let scope = AuthorizationScope {
        brand_ids: [brand.to_string()].into(),
        ..Default::default()
    };
    let record = value(read(&core, &input, &scope, &c).await).await;
    export("get_business_master_record", c.trace_id, &record);
    assert_eq!(record["items"][0]["version"], 3);
    assert_eq!(record["items"][0]["name"], "Name only edit");
    assert!(record["items"][0]["barcode"].is_null());
    let denied = AuthorizationScope {
        brand_ids: [Uuid::new_v4().to_string()].into(),
        ..Default::default()
    };
    assert_eq!(
        read(&core, &input, &denied, &c).await.status(),
        StatusCode::FORBIDDEN
    );
    assert!(READ_TOOLS.contains(&"get_business_master_record"));
    let preserved: (i64,i32) = sqlx::query_as("SELECT credit_limit_minor,payment_terms_days FROM business_customers WHERE code='MA_CUSTOMER'").fetch_one(&pool).await.unwrap();
    assert_eq!(preserved, (34567, 45));
    let address: String =
        sqlx::query_scalar("SELECT address FROM business_warehouses WHERE code='MA_WAREHOUSE'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(address, "KEEP_ADDRESS");
    let zero_cost: bool =
        sqlx::query_scalar("SELECT allow_zero_cost FROM business_products WHERE id=$1")
            .bind(product)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(zero_cost);
    let completed: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM business_document_approval_requests WHERE status='executed'),(SELECT count(*) FROM business_document_approval_votes),(SELECT count(*) FROM business_core_audit_events WHERE operation IN ('CORE_MASTER_DATA_SAVED','PRODUCT_MASTER_DATA_SAVED'))").fetch_one(&pool).await.unwrap();
    assert_eq!(completed, (24, 24, 24));
    let browser_entries = entries
        .iter()
        .map(|(kind, id, _)| (*kind, *id))
        .collect::<Vec<_>>();
    browser_test::check(&core, &pool, actor, &browser_cookie, &browser_entries).await;
    lookup_test::check(&core, &pool, actor, brand).await;
    server.abort();
}
