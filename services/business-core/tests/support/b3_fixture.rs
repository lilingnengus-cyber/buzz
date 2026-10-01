use super::*;

pub(super) async fn create_order(
    service: &PurchasingService,
    f: &Fixture,
    date: NaiveDate,
    key: &str,
    quantity: &str,
    unit_price: &str,
) -> business_core::b3::model::CommandResult {
    service
        .create_order(
            f.actor,
            Uuid::new_v4(),
            key,
            &CreatePurchaseOrder {
                legal_entity_id: f.legal_entity,
                supplier_id: f.supplier,
                buyer_user_id: Some(f.actor),
                business_unit_id: f.business_unit,
                department_id: None,
                brand_id: None,
                currency: "CNY".into(),
                order_date: date,
                expected_delivery_date: Some(date),
                payment_terms_days: Some(30),
                supplier_reference: None,
                business_note: None,
                lines: vec![PurchaseOrderLineInput {
                    sku_id: f.sku,
                    warehouse_id: f.warehouse,
                    unit_of_measure_id: f.uom,
                    quantity: dec(quantity),
                    unit_price: dec(unit_price),
                    discount_amount: dec("0"),
                    tax_rate: dec("0"),
                    business_unit_id: None,
                    department_id: None,
                    brand_id: None,
                }],
            },
        )
        .await
        .unwrap()
}

pub(super) async fn create_receipt(
    service: &ReceivingService,
    f: &Fixture,
    date: NaiveDate,
    order_id: Uuid,
    line_id: Uuid,
    quantity: &str,
    key: &str,
) -> business_core::b3::model::CommandResult {
    service
        .create_receipt(
            f.actor,
            Uuid::new_v4(),
            key,
            &CreateGoodsReceipt {
                purchase_order_id: order_id,
                warehouse_id: f.warehouse,
                receipt_date: date,
                lines: vec![GoodsReceiptLineInput {
                    purchase_order_line_id: line_id,
                    quantity: dec(quantity),
                }],
            },
        )
        .await
        .unwrap()
}

pub(super) async fn assert_balance(
    pool: &sqlx::PgPool,
    f: &Fixture,
    quantity: &str,
    value: &str,
    average: &str,
) {
    let row = sqlx::query("SELECT on_hand_quantity,inventory_value,average_unit_cost FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3")
        .bind(f.legal_entity).bind(f.warehouse).bind(f.sku).fetch_one(pool).await.unwrap();
    assert_eq!(row.get::<Decimal, _>("on_hand_quantity"), decimal(quantity));
    assert_eq!(row.get::<Decimal, _>("inventory_value"), decimal(value));
    assert_eq!(row.get::<Decimal, _>("average_unit_cost"), decimal(average));
}

pub(super) fn version(expected_version: i64) -> VersionCommand {
    VersionCommand {
        expected_version,
        reason_code: Some("TEST".into()),
    }
}

pub(super) fn b2_version(expected_version: i64) -> B2VersionCommand {
    B2VersionCommand {
        expected_version,
        reason_code: Some("TEST".into()),
    }
}

pub(super) fn dec(value: &str) -> DecimalString {
    DecimalString(decimal(value))
}

pub(super) fn decimal(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}

pub(super) async fn seed(pool: &sqlx::PgPool) -> Fixture {
    let f = Fixture {
        actor: Uuid::new_v4(),
        legal_entity: Uuid::new_v4(),
        business_unit: Uuid::new_v4(),
        warehouse: Uuid::new_v4(),
        supplier: Uuid::new_v4(),
        uom: Uuid::new_v4(),
        sku: Uuid::new_v4(),
    };
    let category = Uuid::new_v4();
    let product = Uuid::new_v4();
    let role = Uuid::new_v4();
    let compatibility_legal = Uuid::new_v4();
    let supplier_unit = Uuid::new_v4();
    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'https://issuer.test',$2,'B3 Operator')").bind(f.actor).bind(f.actor.to_string()).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_group_profile(id,code,name,base_currency,timezone) VALUES($1,'B3_GROUP','B3 Test Group','CNY','Asia/Shanghai')").bind(Uuid::new_v4()).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_legal_entities(id,code,name,country_code,functional_currency) VALUES($1,'LE_B3','B3 Legal','CN','CNY')").bind(f.legal_entity).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_legal_entities(id,code,name,country_code,functional_currency) VALUES($1,'LE_B3_COMPAT','B3 Compatibility Legal','CN','CNY')").bind(compatibility_legal).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_units(id,legal_entity_id,code,name) VALUES($1,$2,'BU_B3','B3 Trade')",
    )
    .bind(f.business_unit)
    .bind(compatibility_legal)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_units(id,legal_entity_id,code,name) VALUES($1,$2,'BU_B3_SUPPLIER','B3 Supplier Unit')")
        .bind(supplier_unit)
        .bind(f.legal_entity)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO business_warehouses(id,legal_entity_id,business_unit_id,code,name) VALUES($1,$2,$3,'WH_B3','B3 Warehouse')").bind(f.warehouse).bind(f.legal_entity).bind(f.business_unit).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_suppliers(id,legal_entity_id,business_unit_id,code,name,payment_terms_days) VALUES($1,$2,$3,'SUP_B3','B3 Supplier',30)").bind(f.supplier).bind(f.legal_entity).bind(supplier_unit).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_units_of_measure(id,code,name,precision_scale) VALUES($1,'UOM_B3','Each',0)").bind(f.uom).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_product_categories(id,code,name) VALUES($1,'CAT_B3','B3 Category')",
    )
    .bind(category)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_products(id,code,name,category_id,base_uom_id) VALUES($1,'PROD_B3','B3 Product',$2,$3)").bind(product).bind(category).bind(f.uom).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_skus(id,product_id,code,name) VALUES($1,$2,'SKU_B3','B3 SKU')",
    )
    .bind(f.sku)
    .bind(product)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO business_roles(id,role_key,name) VALUES($1,'b3_operator','B3 Operator')",
    )
    .bind(role)
    .execute(pool)
    .await
    .unwrap();
    for permission in [
        "purchase_order:read",
        "purchase_order:create",
        "purchase_order:update_draft",
        "purchase_order:confirm",
        "purchase_order:cancel_remaining",
        "goods_receipt:read",
        "goods_receipt:create",
        "goods_receipt:confirm",
        "goods_receipt:reverse",
        "inventory:read",
        "inventory_opening:create",
        "inventory_opening:post",
        "payable:read",
        "supplier_payment:read",
        "supplier_payment:create",
        "supplier_payment:confirm",
        "supplier_payment:reverse",
        "payable_allocation:create",
        "payable_allocation:reverse",
    ] {
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
    .bind(f.actor)
    .bind(role)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_legal_entity_scopes(enterprise_user_id,legal_entity_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.legal_entity).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_warehouse_scopes(enterprise_user_id,warehouse_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.warehouse).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_supplier_scopes(enterprise_user_id,supplier_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.supplier).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.business_unit).execute(pool).await.unwrap();
    f
}
