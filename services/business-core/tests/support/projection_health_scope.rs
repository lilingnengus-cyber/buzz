use business_core::s1::{GenerateOperatingSnapshot, OperationsService};
use chrono::NaiveDate;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;
async fn health(service: &OperationsService, actor: Uuid) -> (i64, i64) {
    let quality = service.data_quality(actor).await.unwrap();
    let dashboard = service.dashboard(actor, "2026-01", "CNY").await.unwrap();
    let q = &quality["projection"];
    assert_eq!(
        q["pendingEvents"],
        dashboard["reportHealth"]["pendingEvents"]
    );
    assert_eq!(
        q["pendingFailures"],
        dashboard["reportHealth"]["pendingFailures"]
    );
    (
        q["pendingEvents"].as_i64().unwrap(),
        q["pendingFailures"].as_i64().unwrap(),
    )
}
pub async fn verify(pool: &PgPool, service: &OperationsService, actor: Uuid) {
    // This fixture exercises actual source documents, not arbitrary event IDs.
    let shipment:Uuid=sqlx::query_scalar("SELECT s.id FROM shipments s JOIN sales_orders o ON o.id=s.sales_order_id WHERE o.brand_id IS NOT NULL ORDER BY s.id LIMIT 1").fetch_one(pool).await.unwrap();
    let ret = Uuid::new_v4();
    sqlx::query("INSERT INTO sales_returns(id,return_number,shipment_id,sales_order_id,receivable_id,legal_entity_id,warehouse_id,customer_id,return_date,currency,reason_code,created_by_user_id,trace_id) SELECT $1,'SCOPE-QUALITY-RETURN',s.id,s.sales_order_id,r.id,s.legal_entity_id,s.warehouse_id,s.customer_id,'2026-01-19',s.currency,'scope_test',$2,$3 FROM shipments s JOIN trade_receivables r ON r.shipment_id=s.id WHERE s.id=$4").bind(ret).bind(actor).bind(Uuid::new_v4()).bind(shipment).execute(pool).await.unwrap();
    sqlx::query("DELETE FROM profit_projection_offsets WHERE consumer_name='profit_projection_v1'")
        .execute(pool)
        .await
        .unwrap();
    let (baseline, failures) = health(service, actor).await;
    let expected:i64=sqlx::query_scalar("SELECT count(*) FROM business_core_outbox WHERE topic IN ('shipment_confirmed','shipment_reversed','sales_return_confirmed','sales_return_reversed')").fetch_one(pool).await.unwrap();
    assert!(expected > 0);
    assert_eq!(
        baseline, expected,
        "uninitialized cursor must not hide pending events"
    );
    let mut inserted = Vec::new();
    for (kind, id, topic) in [
        ("shipment", shipment, "shipment_confirmed"),
        ("shipment", shipment, "shipment_reversed"),
        ("sales_return", ret, "sales_return_confirmed"),
        ("sales_return", ret, "sales_return_reversed"),
    ] {
        let event = Uuid::new_v4();
        inserted.push(event);
        sqlx::query("INSERT INTO business_core_outbox(id,topic,aggregate_type,aggregate_id,payload) VALUES($1,$2,$3,$4,'{}')").bind(event).bind(topic).bind(kind).bind(id.to_string()).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO profit_projection_failures(id,outbox_event_id,topic,aggregate_id,error_code,error_summary,trace_id) VALUES($1,$2,$3,$4,'TEST','isolated projection failure',$5)").bind(Uuid::new_v4()).bind(event).bind(topic).bind(id).bind(Uuid::new_v4()).execute(pool).await.unwrap();
    }
    assert_eq!(health(service, actor).await, (baseline + 4, failures + 4));
    // Unmapped or malformed source IDs must not leak global queue activity.
    sqlx::query("INSERT INTO business_core_outbox(id,topic,aggregate_type,aggregate_id,payload) VALUES($1,'shipment_confirmed','shipment','not-a-uuid','{}')").bind(Uuid::new_v4()).execute(pool).await.unwrap();
    assert_eq!(health(service, actor).await, (baseline + 4, failures + 4));
    for (table, column) in [
        ("business_legal_entity_scopes", "legal_entity_id"),
        ("business_customer_scopes", "customer_id"),
        ("business_warehouse_scopes", "warehouse_id"),
        ("business_brand_scopes", "brand_id"),
        ("business_unit_scopes", "business_unit_id"),
    ] {
        let sql = format!(
            "DELETE FROM {table} WHERE enterprise_user_id=$1 RETURNING {column},granted_by"
        );
        let removed: Vec<(Uuid, Uuid)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(actor)
            .fetch_all(pool)
            .await
            .unwrap();
        assert!(!removed.is_empty());
        assert_eq!(health(service, actor).await, (0, 0), "{table}");
        for (id, granter) in removed {
            let sql = format!(
                "INSERT INTO {table}(enterprise_user_id,{column},granted_by) VALUES($1,$2,$3)"
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(actor)
                .bind(id)
                .bind(granter)
                .execute(pool)
                .await
                .unwrap();
        }
    }
    sqlx::query("INSERT INTO profit_projection_offsets(consumer_name,last_outbox_created_at,last_outbox_event_id,updated_at) SELECT 'profit_projection_v1',created_at,id,now() FROM business_core_outbox ORDER BY created_at DESC,id DESC LIMIT 1").execute(pool).await.unwrap();
    assert_eq!(health(service, actor).await, (0, failures + 4));
    sqlx::query("UPDATE profit_projection_failures SET status='resolved',resolved_at=now() WHERE outbox_event_id=ANY($1) AND topic IN ('shipment_confirmed','shipment_reversed')").bind(&inserted).execute(pool).await.unwrap();
    assert_eq!(health(service, actor).await, (0, failures + 2));
    let preview = service
        .operating_snapshot_preview(
            actor,
            &GenerateOperatingSnapshot {
                cadence: "daily".into(),
                currency: "CNY".into(),
                period_start: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
                utc_offset_minutes: 480,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        preview["dataQualityStatus"],
        Value::String("blocked".into()),
        "return projection failure must prevent a complete report"
    );
    sqlx::query("UPDATE profit_projection_failures SET status='resolved',resolved_at=now() WHERE outbox_event_id=ANY($1)").bind(&inserted).execute(pool).await.unwrap();
    assert_eq!(health(service, actor).await, (0, failures));
}
