use super::Fixture;
use business_core::{
    b2::SalesService,
    b3::{PurchasingService, ReceivingService},
    s1::{GenerateOperatingSnapshot, OperationsService},
    PgStore,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
fn amount(v: &Value, key: &str) -> Decimal {
    v["metrics"][key].as_str().unwrap().parse().unwrap()
}
pub async fn verify(pool: &PgPool, service: &OperationsService, f: &Fixture) {
    let second = Uuid::new_v4();
    sqlx::query("INSERT INTO business_warehouses(id,legal_entity_id,business_unit_id,code,name) VALUES($1,$2,$3,'REPORT_SECOND_WH','Second reporting warehouse')").bind(second).bind(f.legal_entity).bind(f.business_unit).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_warehouse_scopes(enterprise_user_id,warehouse_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(second).execute(pool).await.unwrap();
    seed_orders(pool, f, second).await;
    for cadence in ["daily", "weekly"] {
        let base = GenerateOperatingSnapshot {
            cadence: cadence.into(),
            currency: "CNY".into(),
            period_start: NaiveDate::from_ymd_opt(2026, 9, 7).unwrap(),
            utc_offset_minutes: 480,
            legal_entity_ids: None,
            business_unit_ids: None,
            warehouse_ids: None,
        };
        assert!(serde_json::to_value(&base)
            .unwrap()
            .get("warehouseIds")
            .is_none());
        let full = service
            .operating_snapshot_preview(f.actor, &base)
            .await
            .unwrap();
        assert_eq!(full["metrics"]["salesOrderCount"], 1);
        assert_eq!(full["metrics"]["purchaseOrderCount"], 1);
        for (ids, expected) in [
            (vec![f.warehouse], "149"),
            (vec![second], "203.4"),
            (vec![f.warehouse, second, second], "352.4"),
        ] {
            let input = GenerateOperatingSnapshot {
                warehouse_ids: Some(ids.clone()),
                ..base.clone()
            };
            let preview = service
                .operating_snapshot_preview(f.actor, &input)
                .await
                .unwrap();
            assert_eq!(preview["schemaVersion"], 3);
            assert_ne!(preview["scopeHash"], full["scopeHash"]);
            for key in ["salesOrderAmount", "purchaseOrderAmount"] {
                assert_eq!(
                    amount(&preview, key),
                    expected.parse::<Decimal>().unwrap(),
                    "{key}"
                );
            }
            for key in ["salesOrderCount", "purchaseOrderCount"] {
                assert_eq!(
                    preview["metrics"][key], 1,
                    "multiple lines and warehouses must not duplicate orders"
                );
            }
            assert_eq!(
                preview["metrics"]["aggregationBasis"]["orderAmounts"],
                "selected_warehouse_lines"
            );
            assert_eq!(
                preview["metrics"]["aggregationBasis"]["businessUnitFilterApplied"],
                false
            );
            assert_eq!(
                preview["metrics"]["unavailableMetrics"]["slaBreached"],
                "not_attributable_to_selected_warehouses"
            );
            assert!(preview["metrics"]["slaBreached"].is_null());
            if ids == vec![second] {
                assert_eq!(
                    amount(&preview, "inventoryValueAsOfGeneration"),
                    Decimal::from(180)
                );
            }
            let key = format!("wh-{cadence}-{expected}");
            let saved = service
                .generate_operating_snapshot_guarded(
                    f.actor,
                    Uuid::new_v4(),
                    &key,
                    &input,
                    &preview,
                )
                .await
                .unwrap();
            assert_eq!(
                service
                    .generate_operating_snapshot_guarded(
                        f.actor,
                        Uuid::new_v4(),
                        &key,
                        &input,
                        &preview
                    )
                    .await
                    .unwrap(),
                saved
            );
            let detail = service
                .operating_snapshot_detail(f.actor, saved["id"].as_str().unwrap().parse().unwrap())
                .await
                .unwrap();
            assert_eq!(detail["metrics"], preview["metrics"]);
            assert_eq!(detail["scope"], preview["scope"]);
            // Combining with an explicit full unit selection has its own frozen attribution.
            let combined = GenerateOperatingSnapshot {
                business_unit_ids: Some(vec![f.business_unit]),
                ..input
            };
            let both = service
                .operating_snapshot_preview(f.actor, &combined)
                .await
                .unwrap();
            assert_eq!(
                amount(&both, "salesOrderAmount"),
                amount(&preview, "salesOrderAmount")
            );
            assert_ne!(both["scopeHash"], preview["scopeHash"]);
            assert_eq!(
                both["metrics"]["aggregationBasis"]["businessUnitFilterApplied"],
                true
            );
        }
        for ids in [vec![], vec![Uuid::new_v4()]] {
            let invalid = GenerateOperatingSnapshot {
                warehouse_ids: Some(ids),
                ..base.clone()
            };
            assert!(service
                .operating_snapshot_preview(f.actor, &invalid)
                .await
                .is_err());
        }
        // Existing actual shipments belong only to the original warehouse.
        let shipping = GenerateOperatingSnapshot {
            period_start: NaiveDate::from_ymd_opt(
                2026,
                8,
                if cadence == "daily" { 21 } else { 17 },
            )
            .unwrap(),
            warehouse_ids: Some(vec![f.warehouse]),
            ..base.clone()
        };
        let original = service
            .operating_snapshot_preview(f.actor, &shipping)
            .await
            .unwrap();
        assert!(amount(&original, "shippedRevenue") > Decimal::ZERO);
        let other = GenerateOperatingSnapshot {
            warehouse_ids: Some(vec![second]),
            ..shipping
        };
        let empty = service
            .operating_snapshot_preview(f.actor, &other)
            .await
            .unwrap();
        assert_eq!(amount(&empty, "shippedRevenue"), Decimal::ZERO);
        assert_eq!(amount(&empty, "managementOperatingProfit"), Decimal::ZERO);
    }
    verify_comparison(pool, service, f, second).await;
}

async fn seed_orders(pool: &PgPool, f: &Fixture, second: Uuid) {
    let supplier = Uuid::new_v4();
    let sku = Uuid::new_v4();
    sqlx::query("INSERT INTO business_skus(id,product_id,code,name) SELECT $1,product_id,'REPORT_SECOND_SKU','Second reporting SKU' FROM business_skus WHERE id=$2").bind(sku).bind(f.sku).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_suppliers(id,legal_entity_id,business_unit_id,code,name,payment_terms_days) VALUES($1,$2,$3,'REPORT_SUPPLIER','Reporting supplier',30)").bind(supplier).bind(f.legal_entity).bind(f.business_unit).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_supplier_scopes(enterprise_user_id,supplier_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(supplier).execute(pool).await.unwrap();
    for permission in [
        "purchase_order:read",
        "purchase_order:create",
        "purchase_order:confirm",
        "goods_receipt:create",
        "goods_receipt:confirm",
        "goods_receipt:read",
    ] {
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,$2 FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).bind(permission).execute(pool).await.unwrap();
    }
    let lines = vec![
        json!({"skuId":f.sku,"warehouseId":f.warehouse,"unitOfMeasureId":f.uom,"quantity":"1","unitPrice":"100","discountAmount":"10","taxRate":"0.1"}),
        json!({"skuId":sku,"warehouseId":f.warehouse,"unitOfMeasureId":f.uom,"quantity":"1","unitPrice":"50","discountAmount":"0","taxRate":"0"}),
        json!({"skuId":f.sku,"warehouseId":second,"unitOfMeasureId":f.uom,"quantity":"1","unitPrice":"200","discountAmount":"20","taxRate":"0.13"}),
    ];
    let sales = SalesService::new(PgStore::new(pool.clone()), "SO".into(), "SHP".into(), 30);
    sales.create_order(f.actor,Uuid::new_v4(),"report-multi-wh-sales",&serde_json::from_value(json!({"legalEntityId":f.legal_entity,"customerId":f.customer,"salespersonUserId":f.actor,"businessUnitId":f.business_unit,"currency":"CNY","orderDate":"2026-09-07","lines":lines})).unwrap()).await.unwrap();
    let purchasing = PurchasingService::new(PgStore::new(pool.clone()), "PO".into(), 30);
    let order=purchasing.create_order(f.actor,Uuid::new_v4(),"report-multi-wh-purchase",&serde_json::from_value(json!({"legalEntityId":f.legal_entity,"supplierId":supplier,"buyerUserId":f.actor,"businessUnitId":f.business_unit,"currency":"CNY","orderDate":"2026-09-07","lines":lines})).unwrap()).await.unwrap();
    purchasing
        .confirm_order(
            f.actor,
            Uuid::new_v4(),
            order.id,
            "report-multi-wh-purchase-confirm",
            &serde_json::from_value(json!({"expectedVersion":1})).unwrap(),
        )
        .await
        .unwrap();
    let line: Uuid = sqlx::query_scalar(
        "SELECT id FROM purchase_order_lines WHERE purchase_order_id=$1 AND warehouse_id=$2",
    )
    .bind(order.id)
    .bind(second)
    .fetch_one(pool)
    .await
    .unwrap();
    let receiving = ReceivingService::new(
        PgStore::new(pool.clone()),
        purchasing,
        "GR".into(),
        "AP".into(),
    );
    let receipt=receiving.create_receipt(f.actor,Uuid::new_v4(),"report-second-wh-receipt",&serde_json::from_value(json!({"purchaseOrderId":order.id,"warehouseId":second,"receiptDate":"2026-09-07","lines":[{"purchaseOrderLineId":line,"quantity":"1"}]})).unwrap()).await.unwrap();
    receiving
        .confirm_receipt(
            f.actor,
            Uuid::new_v4(),
            receipt.id,
            "report-second-wh-receipt-confirm",
            &serde_json::from_value(json!({"expectedVersion":1})).unwrap(),
        )
        .await
        .unwrap();
}

async fn verify_comparison(pool: &PgPool, service: &OperationsService, f: &Fixture, second: Uuid) {
    let input = GenerateOperatingSnapshot {
        cadence: "daily".into(),
        currency: "CNY".into(),
        period_start: NaiveDate::from_ymd_opt(2026, 9, 8).unwrap(),
        utc_offset_minutes: 480,
        legal_entity_ids: None,
        business_unit_ids: Some(vec![f.business_unit]),
        warehouse_ids: Some(vec![f.warehouse, second]),
    };
    let combined = service
        .generate_operating_snapshot(f.actor, Uuid::new_v4(), "wh-compare-both-filters", &input)
        .await
        .unwrap();
    let next = GenerateOperatingSnapshot {
        period_start: NaiveDate::from_ymd_opt(2026, 9, 9).unwrap(),
        business_unit_ids: None,
        ..input.clone()
    };
    let report = service
        .generate_operating_snapshot(f.actor, Uuid::new_v4(), "wh-compare-warehouse-only", &next)
        .await
        .unwrap();
    let series = service
        .operating_trends(f.actor, "daily", "CNY", 60)
        .await
        .unwrap();
    let rows = series["items"].as_array().unwrap();
    let combined_row = rows.iter().find(|r| r["id"] == combined["id"]).unwrap();
    assert!(combined_row["comparisonSnapshotId"].is_null());
    let next_row = rows.iter().find(|r| r["id"] == report["id"]).unwrap();
    let previous = rows
        .iter()
        .find(|r| r["id"] == next_row["comparisonSnapshotId"])
        .unwrap();
    assert_eq!(previous["periodStart"], "2026-09-07");
    assert_eq!(
        previous["metrics"]["aggregationBasis"],
        next_row["metrics"]["aggregationBasis"]
    );
    sqlx::query(
        "DELETE FROM business_warehouse_scopes WHERE enterprise_user_id=$1 AND warehouse_id=$2",
    )
    .bind(f.actor)
    .bind(second)
    .execute(pool)
    .await
    .unwrap();
    assert!(service
        .generate_operating_snapshot(f.actor, Uuid::new_v4(), "wh-compare-warehouse-only", &next)
        .await
        .is_err());
    assert!(service
        .operating_snapshot_detail(f.actor, report["id"].as_str().unwrap().parse().unwrap())
        .await
        .is_err());
    sqlx::query("INSERT INTO business_warehouse_scopes(enterprise_user_id,warehouse_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(second).execute(pool).await.unwrap();
    let foreign = Uuid::new_v4();
    sqlx::query("INSERT INTO business_warehouses(id,legal_entity_id,business_unit_id,code,name) SELECT $1,legal_entity_id,id,'REPORT_FOREIGN_WH','Foreign reporting warehouse' FROM business_units WHERE code='REPORT_OTHER_LE_UNIT'").bind(foreign).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_warehouse_scopes(enterprise_user_id,warehouse_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(foreign).execute(pool).await.unwrap();
    let invalid = GenerateOperatingSnapshot {
        legal_entity_ids: Some(vec![f.legal_entity]),
        warehouse_ids: Some(vec![foreign]),
        ..input
    };
    assert!(service
        .operating_snapshot_preview(f.actor, &invalid)
        .await
        .is_err());
    sqlx::query(
        "DELETE FROM business_warehouse_scopes WHERE enterprise_user_id=$1 AND warehouse_id=$2",
    )
    .bind(f.actor)
    .bind(foreign)
    .execute(pool)
    .await
    .unwrap();
}
