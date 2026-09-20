use business_core::s1::{GenerateOperatingSnapshot, OperationsService};
use chrono::NaiveDate;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
pub async fn verify(pool: &PgPool, service: &OperationsService, actor: Uuid) {
    let input = GenerateOperatingSnapshot {
        cadence: "daily".into(),
        currency: "CNY".into(),
        period_start: NaiveDate::from_ymd_opt(2025, 1, 6).unwrap(),
        legal_entity_ids: None,
        utc_offset_minutes: 480,
    };
    let result = service
        .generate_operating_snapshot(actor, Uuid::new_v4(), "recorded-scope-detail", &input)
        .await
        .unwrap();
    let id: Uuid = result["id"].as_str().unwrap().parse().unwrap();
    let original = service.operating_snapshot_detail(actor, id).await.unwrap();
    assert_eq!(original["scopeBasis"], "recorded");
    assert_eq!(original["ownerUserId"], json!(actor));
    let legacy = Uuid::new_v4();
    sqlx::query("INSERT INTO operating_report_snapshots(id,cadence,period_start,period_end,currency,scope_hash,payload,data_quality_status,source_hash,generated_by_user_id,trace_id,utc_offset_minutes) SELECT $1,cadence,period_start,period_end,currency,scope_hash,payload,data_quality_status,source_hash,generated_by_user_id,trace_id,NULL FROM operating_report_snapshots WHERE id=$2").bind(legacy).bind(id).execute(pool).await.unwrap();
    assert_eq!(
        service
            .operating_snapshot_detail(actor, legacy)
            .await
            .unwrap()["scopeBasis"],
        "legacy_current_identity"
    );
    // Unrelated authorization revision must not erase a known historical report.
    sqlx::query("UPDATE business_role_permissions SET permission_key=permission_key WHERE permission_key='management_report:read'")
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(
        service.operating_snapshot_detail(actor, id).await.unwrap(),
        original
    );
    assert!(service
        .operating_snapshot_detail(actor, legacy)
        .await
        .is_err());
    let trends = service
        .operating_trends(actor, "daily", "CNY", 60)
        .await
        .unwrap();
    assert!(trends["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["id"] == json!(id)));
    for (table, column) in [
        ("business_legal_entity_scopes", "legal_entity_id"),
        ("business_customer_scopes", "customer_id"),
        ("business_brand_scopes", "brand_id"),
        ("business_unit_scopes", "business_unit_id"),
        ("business_warehouse_scopes", "warehouse_id"),
        ("business_supplier_scopes", "supplier_id"),
    ] {
        let sql = format!(
            "DELETE FROM {table} WHERE enterprise_user_id=$1 RETURNING {column},granted_by"
        );
        let removed: Vec<(Uuid, Uuid)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(actor)
            .fetch_all(pool)
            .await
            .unwrap();
        if removed.is_empty() {
            continue;
        }
        assert!(
            service.operating_snapshot_detail(actor, id).await.is_err(),
            "{table}"
        );
        let trends = service
            .operating_trends(actor, "daily", "CNY", 60)
            .await
            .unwrap();
        assert!(
            !trends["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["id"] == json!(id)),
            "{table}"
        );
        for (dimension, granter) in removed {
            let sql = format!(
                "INSERT INTO {table}(enterprise_user_id,{column},granted_by) VALUES($1,$2,$3)"
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(actor)
                .bind(dimension)
                .bind(granter)
                .execute(pool)
                .await
                .unwrap();
        }
        assert_eq!(
            service.operating_snapshot_detail(actor, id).await.unwrap(),
            original
        );
    }
    let roles:Vec<Uuid>=sqlx::query_scalar("DELETE FROM business_role_permissions WHERE permission_key='management_report:read' RETURNING role_id").fetch_all(pool).await.unwrap();
    assert!(service.operating_snapshot_detail(actor, id).await.is_err());
    for role in roles {
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) VALUES($1,'management_report:read')").bind(role).execute(pool).await.unwrap();
    }
    // Comparisons require the same frozen scope, even when both reports are visible.
    let extra = Uuid::new_v4();
    sqlx::query("INSERT INTO business_brands(id,code,name) VALUES($1,'DETAIL_SCOPE_EXTRA','Additional report scope')").bind(extra).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)").bind(actor).bind(extra).execute(pool).await.unwrap();
    let expanded = GenerateOperatingSnapshot {
        period_start: NaiveDate::from_ymd_opt(2025, 1, 13).unwrap(),
        ..input.clone()
    };
    let expanded_report = service
        .generate_operating_snapshot(actor, Uuid::new_v4(), "expanded-report-scope", &expanded)
        .await
        .unwrap();
    sqlx::query("UPDATE business_role_permissions SET permission_key=permission_key WHERE permission_key='management_report:read'").execute(pool).await.unwrap();
    let next = GenerateOperatingSnapshot {
        period_start: NaiveDate::from_ymd_opt(2025, 1, 20).unwrap(),
        ..input.clone()
    };
    let next_report = service
        .generate_operating_snapshot(
            actor,
            Uuid::new_v4(),
            "same-recorded-scope-new-revision",
            &next,
        )
        .await
        .unwrap();
    let series = service
        .operating_trends(actor, "daily", "CNY", 60)
        .await
        .unwrap();
    let items = series["items"].as_array().unwrap();
    let expanded_row = items
        .iter()
        .find(|r| r["id"] == expanded_report["id"])
        .unwrap();
    assert!(expanded_row["comparisonSnapshotId"].is_null());
    let next_row = items.iter().find(|r| r["id"] == next_report["id"]).unwrap();
    assert_eq!(next_row["comparisonSnapshotId"], expanded_report["id"]);
    sqlx::query("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2")
        .bind(actor)
        .bind(extra)
        .execute(pool)
        .await
        .unwrap();
    assert!(service
        .operating_snapshot_detail(
            actor,
            expanded_report["id"].as_str().unwrap().parse().unwrap()
        )
        .await
        .is_err());
    let stored: Value =
        sqlx::query_scalar("SELECT snapshot_scope FROM operating_report_snapshots WHERE id=$1")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(stored, original["scope"]);
    assert_eq!(
        service.operating_snapshot_detail(actor, id).await.unwrap(),
        original
    );
}
