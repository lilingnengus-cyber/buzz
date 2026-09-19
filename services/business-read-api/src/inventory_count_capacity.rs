use super::*;

async fn measure(label: &str, response: Response) -> Value {
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 32 * 1024 * 1024)
        .await
        .unwrap();
    eprintln!("count capacity {label}: {} bytes", bytes.len());
    assert_eq!(status, StatusCode::OK);
    if let Ok(directory) = std::env::var("BUSINESS_COUNT_CAPACITY_OUTPUT_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            std::path::Path::new(&directory).join(format!("{label}.json")),
            &bytes,
        )
        .unwrap();
    }

    assert!(
        bytes.len() < 1024 * 1024,
        "{label}: {} bytes exceeds 1 MiB",
        bytes.len()
    );
    serde_json::from_slice(&bytes).unwrap()
}
#[tokio::test]
async fn five_hundred_line_count_and_multi_document_search_fit_transport() {
    let Ok(url) = std::env::var("BUSINESS_COUNT_CAPACITY_DATABASE_URL") else {
        eprintln!("BUSINESS_COUNT_CAPACITY_DATABASE_URL absent; count capacity test skipped");
        return;
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(12)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool);
    store.migrate().await.unwrap();
    let f = b2_seed::seed(store.pool()).await;
    let skus:Vec<Uuid>=sqlx::query_scalar("INSERT INTO business_skus(id,product_id,code,name) SELECT gen_random_uuid(),s.product_id,'COUNT-CAPACITY-'||n,repeat('商',200) FROM business_skus s CROSS JOIN generate_series(1,500) n WHERE s.id=$1 RETURNING id").bind(f.sku).fetch_all(store.pool()).await.unwrap();
    sqlx::query("INSERT INTO inventory_balances(legal_entity_id,warehouse_id,sku_id) SELECT $1,$2,unnest($3::uuid[])").bind(f.legal_entity).bind(f.warehouse).bind(&skus).execute(store.pool()).await.unwrap();
    for action in [
        "inventory_opening:create",
        "inventory_opening:post",
        "inventory_opening:reverse",
    ] {
        sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval,require_distinct_business_unit) VALUES($1,$1,ARRAY['b2_operator'],1,true,false)").bind(action).execute(store.pool()).await.unwrap();
    }
    let (core, task) = serve(business_core::api::router(
        business_core::api::AppState::new(
            store.clone(),
            &config(url, "count-test-credential".into()),
        ),
    ))
    .await;
    let mut ctx = context("inventory_count_creation_intent:create");
    ctx.enterprise_user_id = f.actor;
    let create = json!({"legalEntityId":f.legal_entity,"warehouseId":f.warehouse,"countDate":"2026-09-20","currency":"CNY","skuIds":skus,"businessNote":"备".repeat(1000)});
    let prepared = measure(
        "creation",
        forward(
            &core,
            "prepare_inventory_count_creation",
            create,
            &ctx,
            &authority(&ctx, &f),
        )
        .await,
    )
    .await;
    let created = approve(&core, &f, "creation", &prepared, "approve").await;
    let id: Uuid = created["createdDocument"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let mut read_ctx = context("inventory:read");
    read_ctx.enterprise_user_id = f.actor;
    let scope = iam_authorization_scope(&authority(&read_ctx, &f), "inventory:read").unwrap();
    let detail = measure(
        "detail",
        inventory_counts::read(
            &core,
            "get_inventory_count",
            &json!({"documentId":id}),
            &scope,
            &read_ctx,
        )
        .await,
    )
    .await;
    assert_eq!(detail["items"][0]["lines"].as_array().unwrap().len(), 500);
    let lines=detail["items"][0]["lines"].as_array().unwrap().iter().map(|line|json!({"countLineId":line["id"],"actualOnHandQuantity":"1","surplusUnitCost":"7"})).collect::<Vec<_>>();
    let mut ctx = context("inventory_count_submission_intent:create");
    ctx.enterprise_user_id = f.actor;
    let prepared = measure(
        "submission",
        forward(
            &core,
            "prepare_inventory_count_submission",
            json!({"inventoryCountId":id,"command":{"expectedVersion":1,"lines":lines}}),
            &ctx,
            &authority(&ctx, &f),
        )
        .await,
    )
    .await;
    let submitted = approve(&core, &f, "submission", &prepared, "approve").await;
    assert_eq!(submitted["updatedDocument"]["version"], 2);
    let mut ctx = context("inventory_count_posting_intent:create");
    ctx.enterprise_user_id = f.actor;
    let prepared = measure(
        "posting",
        forward(
            &core,
            "prepare_inventory_count_posting",
            json!({"inventoryCountId":id,"command":{"expectedVersion":2}}),
            &ctx,
            &authority(&ctx, &f),
        )
        .await,
    )
    .await;
    let posted = approve(&core, &f, "posting", &prepared, "approve").await;
    assert_eq!(posted["updatedDocument"]["status"], "posted");
    let totals:Value=sqlx::query_scalar("SELECT jsonb_build_object('quantity',sum(on_hand_quantity)::text,'value',sum(inventory_value)::text) FROM inventory_balances WHERE sku_id=ANY($1)").bind(&skus).fetch_one(store.pool()).await.unwrap();
    assert_eq!(
        totals,
        json!({"quantity":"500.000000","value":"3500.000000"})
    );
    // Add read-only history fixtures without creating inventory movements.
    for i in 0..20 {
        let clone = Uuid::new_v4();
        sqlx::query("INSERT INTO inventory_count_tasks(id,count_number,legal_entity_id,warehouse_id,count_date,currency,status,created_by_user_id,trace_id,scope_snapshot_captured,snapshot_business_unit_id) SELECT $2,$3,legal_entity_id,warehouse_id,count_date,currency,'posted',created_by_user_id,trace_id,scope_snapshot_captured,snapshot_business_unit_id FROM inventory_count_tasks WHERE id=$1").bind(id).bind(clone).bind(format!("CAPACITY-HISTORY-{i:03}")).execute(store.pool()).await.unwrap();
        sqlx::query("INSERT INTO inventory_count_lines(id,inventory_count_id,sku_id,snapshot_on_hand_quantity,snapshot_reserved_quantity,snapshot_quarantined_quantity,snapshot_inventory_value,snapshot_average_unit_cost,actual_on_hand_quantity,surplus_unit_cost,variance_quantity,variance_value,snapshot_brand_id) SELECT gen_random_uuid(),$2,sku_id,snapshot_on_hand_quantity,snapshot_reserved_quantity,snapshot_quarantined_quantity,snapshot_inventory_value,snapshot_average_unit_cost,actual_on_hand_quantity,surplus_unit_cost,variance_quantity,variance_value,snapshot_brand_id FROM inventory_count_lines WHERE inventory_count_id=$1").bind(id).bind(clone).execute(store.pool()).await.unwrap();
    }
    let search = measure(
        "search",
        inventory_counts::read(
            &core,
            "search_inventory_counts",
            &json!({"limit":20}),
            &scope,
            &read_ctx,
        )
        .await,
    )
    .await;
    assert_eq!(search["items"].as_array().unwrap().len(), 20);
    assert_eq!(search["pagination"]["hasMore"], true);
    assert!(search["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["lineCount"] == 500 && item.get("lines").is_none()));
    assert!(serde_json::to_vec(&search).unwrap().len() < 50 * 1024);
    let last = measure(
        "search-last-page",
        inventory_counts::read(
            &core,
            "search_inventory_counts",
            &json!({"limit":20,"offset":20}),
            &scope,
            &read_ctx,
        )
        .await,
    )
    .await;
    assert_eq!(last["items"].as_array().unwrap().len(), 1);
    assert_eq!(last["pagination"]["hasMore"], false);
    task.abort();
}
