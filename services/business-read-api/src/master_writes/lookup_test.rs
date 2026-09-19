use super::*;

pub(super) async fn check(core: &CoreClient, pool: &PgPool, actor: Uuid, brand: Uuid) {
    let context = context(actor, "search_business_master_data");
    let unrestricted = AuthorizationScope::default();
    let branded = AuthorizationScope {
        brand_ids: [brand.to_string()].into(),
        ..Default::default()
    };
    for (kind, query, scope) in [
        ("product", "MA_PRODUCT", &branded),
        ("product_category", "MA_CATEGORY", &unrestricted),
        ("uom_conversion", "MA_PRODUCT:MA_BOX", &branded),
    ] {
        let input = json!({"resourceType":kind,"query":query,"limit":1});
        let result = value(crate::master_data::search(core, &input, scope, &context).await).await;
        assert_eq!(result["items"].as_array().unwrap().len(), 1, "{kind}");
        assert_eq!(result["items"][0]["resourceType"], kind);
        assert_eq!(result["items"][0]["version"], 2);
        assert_eq!(result["pagination"]["hasMore"], false);
        let id = result["items"][0]["id"].clone();
        let by_name = value(
            crate::master_data::search(
                core,
                &json!({"resourceType":kind,"query":result["items"][0]["name"]}),
                scope,
                &context,
            )
            .await,
        )
        .await;
        assert!(by_name["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == id));
        let detail_context = super::context(actor, "get_business_master_record");
        let detail = value(
            read(
                core,
                &json!({"resourceType":kind,"documentId":id}),
                scope,
                &detail_context,
            )
            .await,
        )
        .await;
        assert_eq!(detail["items"][0]["id"], id);
        for input in [
            json!({"resourceType":kind,"query":"%"}),
            json!({"resourceType":kind,"query":query,"offset":1}),
        ] {
            let result =
                value(crate::master_data::search(core, &input, scope, &context).await).await;
            assert_eq!(result["items"], json!([]));
        }
        if kind != "product_category" {
            let wrong = AuthorizationScope {
                brand_ids: [Uuid::new_v4().to_string()].into(),
                ..Default::default()
            };
            let result =
                value(crate::master_data::search(core, &input, &wrong, &context).await).await;
            assert_eq!(result["items"], json!([]));
        }
    }
    // Core object authority applies even when the delegated read scope is unrestricted.
    sqlx::query("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2")
        .bind(actor)
        .bind(brand)
        .execute(pool)
        .await
        .unwrap();
    for kind in ["product", "uom_conversion"] {
        let result = value(
            crate::master_data::search(
                core,
                &json!({"resourceType":kind}),
                &unrestricted,
                &context,
            )
            .await,
        )
        .await;
        assert_eq!(result["items"], json!([]));
    }
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)")
        .bind(actor).bind(brand).execute(pool).await.unwrap();
    // Disabled conversions must not become candidates for a new business write.
    sqlx::query("UPDATE business_product_uom_conversions SET status='disabled'")
        .execute(pool)
        .await
        .unwrap();
    let result = value(
        crate::master_data::search(
            core,
            &json!({"resourceType":"uom_conversion"}),
            &branded,
            &context,
        )
        .await,
    )
    .await;
    assert_eq!(result["items"], json!([]));
}
