use super::{Fixture, Uuid};

pub(super) async fn seed(pool: &sqlx::PgPool) -> Fixture {
    let fixture = Fixture {
        actor: Uuid::new_v4(),
        legal_entity: Uuid::new_v4(),
        business_unit: Uuid::new_v4(),
        warehouse: Uuid::new_v4(),
        customer: Uuid::new_v4(),
        brand: Uuid::new_v4(),
        uom: Uuid::new_v4(),
        sku: Uuid::new_v4(),
    };
    let category = Uuid::new_v4();
    let product = Uuid::new_v4();
    let role = Uuid::new_v4();
    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'https://issuer.test',$2,'B2 Operator')").bind(fixture.actor).bind(fixture.actor.to_string()).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_group_profile(id,code,name,base_currency,timezone) VALUES($1,'B2_GROUP','B2 Test Group','CNY','Asia/Shanghai')").bind(Uuid::new_v4()).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_legal_entities(id,code,name,country_code,functional_currency) VALUES($1,'LE_B2','B2 Legal','CN','CNY')").bind(fixture.legal_entity).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_units(id,legal_entity_id,code,name) VALUES($1,$2,'BU_B2','B2 Trade')",
    )
    .bind(fixture.business_unit)
    .bind(fixture.legal_entity)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_warehouses(id,legal_entity_id,business_unit_id,code,name) VALUES($1,$2,$3,'WH_B2','B2 Warehouse')").bind(fixture.warehouse).bind(fixture.legal_entity).bind(fixture.business_unit).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_customers(id,legal_entity_id,business_unit_id,code,name,credit_currency,payment_terms_days) VALUES($1,$2,$3,'CUS_B2','B2 Customer','CNY',30)").bind(fixture.customer).bind(fixture.legal_entity).bind(fixture.business_unit).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_units_of_measure(id,code,name,precision_scale) VALUES($1,'UOM_B2','Each',0)").bind(fixture.uom).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_product_categories(id,code,name) VALUES($1,'CAT_B2','B2 Category')",
    )
    .bind(category)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_brands(id,code,name) VALUES($1,'BRAND_B2','B2 Brand')")
        .bind(fixture.brand)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO business_products(id,code,name,category_id,brand_id,base_uom_id) VALUES($1,'PROD_B2','B2 Product',$2,$3,$4)").bind(product).bind(category).bind(fixture.brand).bind(fixture.uom).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_skus(id,product_id,code,name) VALUES($1,$2,'SKU_B2','B2 SKU')",
    )
    .bind(fixture.sku)
    .bind(product)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO business_roles(id,role_key,name) VALUES($1,'b2_operator','B2 Operator')",
    )
    .bind(role)
    .execute(pool)
    .await
    .unwrap();
    let permissions = [
        "sales_order:read",
        "sales_order:create",
        "sales_order:update_draft",
        "sales_order:confirm",
        "sales_order:cancel",
        "sales_order:place_hold",
        "sales_order:release_hold",
        "shipment:create",
        "shipment:confirm",
        "shipment:reverse",
        "inventory:read",
        "inventory_opening:create",
        "inventory_opening:post",
        "inventory_opening:reverse",
        "receivable:read",
        "customer_receipt:read",
        "customer_receipt:create",
        "customer_receipt:confirm",
        "customer_receipt:reverse",
        "receivable_allocation:create",
        "receivable_allocation:reverse",
    ];
    for permission in permissions {
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,$2)")
            .bind(role)
            .bind(permission)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) VALUES($1,$2,$1)",
    )
    .bind(fixture.actor)
    .bind(role)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_legal_entity_scopes(enterprise_user_id,legal_entity_id,granted_by) VALUES($1,$2,$1)").bind(fixture.actor).bind(fixture.legal_entity).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_warehouse_scopes(enterprise_user_id,warehouse_id,granted_by) VALUES($1,$2,$1)").bind(fixture.actor).bind(fixture.warehouse).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)").bind(fixture.actor).bind(fixture.customer).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)").bind(fixture.actor).bind(fixture.brand).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)").bind(fixture.actor).bind(fixture.business_unit).execute(pool).await.unwrap();
    fixture
}
