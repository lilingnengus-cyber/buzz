use super::*;
use business_core::b2::{CreateInventoryCount, InventoryCountService};

pub(super) async fn check(app: &Router, store: &PgStore, f: &Fixture) {
    let product = Uuid::new_v4();
    let sku = Uuid::new_v4();
    sqlx::query("INSERT INTO business_products(id,code,name,category_id,brand_id,base_uom_id) SELECT $1,'COUNT_LOOKUP_PRODUCT','Count lookup',category_id,brand_id,base_uom_id FROM business_products WHERE id=(SELECT product_id FROM business_skus WHERE id=$2)").bind(product).bind(f.sku).execute(store.pool()).await.unwrap();
    sqlx::query("INSERT INTO business_skus(id,product_id,code,name) VALUES($1,$2,'COUNT_LOOKUP_SKU','盘点查询商品')").bind(sku).bind(product).execute(store.pool()).await.unwrap();
    sqlx::query(
        "INSERT INTO inventory_balances(legal_entity_id,warehouse_id,sku_id) VALUES($1,$2,$3)",
    )
    .bind(f.legal_entity)
    .bind(f.warehouse)
    .bind(sku)
    .execute(store.pool())
    .await
    .unwrap();
    let service = InventoryCountService::new(store.clone(), "IC".into());
    let input:CreateInventoryCount=serde_json::from_value(json!({"legalEntityId":f.legal_entity,"warehouseId":f.warehouse,"countDate":"2026-09-20","currency":"CNY","skuIds":[sku]})).unwrap();
    let options = format!("/v1/agent-inventory-count-options?skuId={sku}&limit=1");
    let (status, available) = call(app, f.actor, "GET", &options, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{available}");
    assert_eq!(available["items"].as_array().unwrap().len(), 1);
    assert_eq!(available["items"][0]["skuId"], json!(sku));
    assert_eq!(available["items"][0]["onHandQuantity"], "0.000000");
    let mut ids = Vec::new();
    for i in 0..3 {
        let count = service
            .create(
                f.actor,
                Uuid::new_v4(),
                &format!("count-lookup-create-{i}"),
                &input,
            )
            .await
            .unwrap();
        ids.push(count.id);
        if i < 2 {
            service
                .cancel(
                    f.actor,
                    Uuid::new_v4(),
                    count.id,
                    &format!("count-lookup-cancel-{i}"),
                    &version(1),
                )
                .await
                .unwrap();
        }
    }
    assert_eq!(
        call(app, f.actor, "GET", &options, Value::Null).await.1["items"],
        json!([])
    );
    let path = format!("/v1/agent-inventory-counts?skuId={sku}");
    let mut seen = std::collections::BTreeSet::new();
    for offset in 0..3 {
        let (status, page) = call(
            app,
            f.actor,
            "GET",
            &format!("{path}&offset={offset}&limit=1"),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let items = page["items"].as_array().unwrap();
        assert_eq!(items.len(), if offset == 2 { 1 } else { 2 });
        assert!(seen.insert(items[0]["id"].as_str().unwrap().to_string()));
        assert_eq!(items[0]["lines"][0]["skuId"], json!(sku));
        assert!(items[0]["lines"][0]["id"].is_string());
        assert!(items[0]["lines"][0]["actualOnHandQuantity"].is_null());
    }
    assert_eq!(seen, ids.iter().map(ToString::to_string).collect());
    let exact = format!("/v1/agent-inventory-counts/{}", ids[2]);
    let (status, detail) = call(app, f.actor, "GET", &exact, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["item"]["status"], "counting");
    assert_eq!(detail["item"]["retainsFreeze"], true);
    assert_eq!(detail["item"]["version"], 1);
    let (_, filtered) = call(
        app,
        f.actor,
        "GET",
        &format!(
            "{path}&status=cancelled&warehouseId={}&legalEntityId={}",
            f.warehouse, f.legal_entity
        ),
        Value::Null,
    )
    .await;
    assert_eq!(filtered["items"].as_array().unwrap().len(), 2);
    let (_, none) = call(
        app,
        f.actor,
        "GET",
        &format!("{path}&warehouseId={}", Uuid::new_v4()),
        Value::Null,
    )
    .await;
    assert_eq!(none["items"], json!([]));
    for invalid in [
        "limit=0",
        "limit=101",
        "offset=100001",
        "status=draft",
        "partyId=x",
        "execute=true",
        "warehouseId=guess",
        "query=%25",
    ] {
        assert!(
            call(
                app,
                f.actor,
                "GET",
                &format!("/v1/agent-inventory-counts?{invalid}"),
                Value::Null
            )
            .await
            .0
            .is_client_error(),
            "{invalid}"
        );
    }
    for invalid in [
        "limit=0",
        "limit=101",
        "offset=100001",
        "status=counting",
        "warehouseId=guess",
    ] {
        assert!(
            call(
                app,
                f.actor,
                "GET",
                &format!("/v1/agent-inventory-count-options?{invalid}"),
                Value::Null
            )
            .await
            .0
            .is_client_error(),
            "{invalid}"
        );
    }
    assert_eq!(
        call(
            app,
            f.actor,
            "GET",
            &format!("/v1/agent-inventory-counts/{}", Uuid::new_v4()),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    service
        .cancel(
            f.actor,
            Uuid::new_v4(),
            ids[2],
            "count-lookup-last-cancel",
            &version(1),
        )
        .await
        .unwrap();
    assert_eq!(
        call(app, f.actor, "GET", &options, Value::Null).await.1["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for (revoke,restore,value) in [
        ("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2","INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)",f.brand),
        ("DELETE FROM business_unit_scopes WHERE enterprise_user_id=$1 AND business_unit_id=$2","INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)",f.business_unit),
        ("DELETE FROM business_warehouse_scopes WHERE enterprise_user_id=$1 AND warehouse_id=$2","INSERT INTO business_warehouse_scopes(enterprise_user_id,warehouse_id,granted_by) VALUES($1,$2,$1)",f.warehouse),
        ("DELETE FROM business_legal_entity_scopes WHERE enterprise_user_id=$1 AND legal_entity_id=$2","INSERT INTO business_legal_entity_scopes(enterprise_user_id,legal_entity_id,granted_by) VALUES($1,$2,$1)",f.legal_entity),
    ] {
        sqlx::query(revoke).bind(f.actor).bind(value).execute(store.pool()).await.unwrap();
        assert_eq!(call(app,f.actor,"GET",&path,Value::Null).await.1["items"],json!([]));
        assert_eq!(call(app,f.actor,"GET",&exact,Value::Null).await.0,StatusCode::NOT_FOUND);
        assert_eq!(call(app,f.actor,"GET",&options,Value::Null).await.1["items"],json!([]));
        sqlx::query(restore).bind(f.actor).bind(value).execute(store.pool()).await.unwrap();
    }
    let new_unit = Uuid::new_v4();
    sqlx::query("INSERT INTO business_units(id,legal_entity_id,code,name) VALUES($1,$2,'COUNT_LOOKUP_UNIT','Count lookup unit')").bind(new_unit).bind(f.legal_entity).execute(store.pool()).await.unwrap();
    sqlx::query("UPDATE business_warehouses SET business_unit_id=$2 WHERE id=$1")
        .bind(f.warehouse)
        .bind(new_unit)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        call(app, f.actor, "GET", &exact, Value::Null).await.0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(new_unit).execute(store.pool()).await.unwrap();
    assert_eq!(
        call(app, f.actor, "GET", &exact, Value::Null).await.0,
        StatusCode::OK
    );
    sqlx::query(
        "DELETE FROM business_unit_scopes WHERE enterprise_user_id=$1 AND business_unit_id=$2",
    )
    .bind(f.actor)
    .bind(f.business_unit)
    .execute(store.pool())
    .await
    .unwrap();
    assert_eq!(
        call(app, f.actor, "GET", &exact, Value::Null).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(app, f.actor, "GET", &path, Value::Null).await.1["items"],
        json!([])
    );
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.business_unit).execute(store.pool()).await.unwrap();
    sqlx::query("UPDATE business_warehouses SET business_unit_id=$2 WHERE id=$1")
        .bind(f.warehouse)
        .bind(f.business_unit)
        .execute(store.pool())
        .await
        .unwrap();
    let new_brand = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO business_brands(id,code,name) VALUES($1,'COUNT_LOOKUP_NEW','New count brand')",
    )
    .bind(new_brand)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE business_products SET brand_id=$2 WHERE id=$1")
        .bind(product)
        .bind(new_brand)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        call(app, f.actor, "GET", &path, Value::Null).await.1["items"],
        json!([])
    );
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(new_brand).execute(store.pool()).await.unwrap();
    assert_eq!(
        call(app, f.actor, "GET", &exact, Value::Null).await.0,
        StatusCode::OK
    );
    sqlx::query("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2")
        .bind(f.actor)
        .bind(f.brand)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        call(app, f.actor, "GET", &exact, Value::Null).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(app, f.actor, "GET", &path, Value::Null).await.1["items"],
        json!([])
    );
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.brand).execute(store.pool()).await.unwrap();
    assert_eq!(
        call(app, f.actor, "GET", &options, Value::Null).await.1["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    sqlx::query("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2")
        .bind(f.actor)
        .bind(new_brand)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        call(app, f.actor, "GET", &options, Value::Null).await.1["items"],
        json!([])
    );
    sqlx::query("UPDATE business_products SET brand_id=$2 WHERE id=$1")
        .bind(product)
        .bind(f.brand)
        .execute(store.pool())
        .await
        .unwrap();
}
