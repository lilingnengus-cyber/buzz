use super::{batch, Fixture};
use business_core::b4::{
    model::{CommandResult, VersionCommand},
    AdjustmentService,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
async fn counts(pool: &PgPool) -> (i64, i64, i64, i64, i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM operational_adjustment_previews),(SELECT count(*) FROM operational_adjustment_allocations),(SELECT count(*) FROM business_core_audit_events),(SELECT count(*) FROM business_command_idempotency),(SELECT count(*) FROM profit_facts),(SELECT count(*) FROM operational_adjustment_events),(SELECT count(*) FROM business_core_outbox)").fetch_one(pool).await.unwrap()
}
async fn draft(
    pool: &PgPool,
    service: &AdjustmentService,
    f: &Fixture,
    key: &str,
) -> CommandResult {
    let orders:Vec<Uuid>=sqlx::query_scalar("SELECT sales_order_id FROM order_profit_current WHERE legal_entity_id=$1 AND net_revenue>0 ORDER BY sales_order_id LIMIT 2").bind(f.legal_entity).fetch_all(pool).await.unwrap();
    assert_eq!(orders.len(), 2);
    service
        .create(
            f.actor,
            Uuid::new_v4(),
            key,
            &batch(
                f,
                NaiveDate::from_ymd_opt(2026, 8, 21).unwrap(),
                "allocated_operating_expense",
                "10.01",
                "net_revenue",
                orders,
            ),
        )
        .await
        .unwrap()
}
pub async fn verify(pool: &PgPool, service: &AdjustmentService, f: &Fixture) {
    let d = draft(pool, service, f, "guarded-adjustment-draft").await;
    let v = VersionCommand {
        expected_version: 1,
    };
    let preview = service.allocation_preview(f.actor, d.id, &v).await.unwrap();
    let before = counts(pool).await;
    let mut tx = pool.begin().await.unwrap();
    assert!(service
        .post_guarded_on(
            &mut tx,
            f.actor,
            Uuid::new_v4(),
            d.id,
            "guarded-invalid-isolation",
            &v,
            &preview
        )
        .await
        .is_err());
    tx.rollback().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await
        .unwrap();
    let result = service
        .post_guarded_on(
            &mut tx,
            f.actor,
            Uuid::new_v4(),
            d.id,
            "guarded-caller-rollback",
            &v,
            &preview,
        )
        .await
        .unwrap();
    assert_eq!(result.status, "posted");
    assert_eq!(counts(pool).await, before, "no nested commit");
    tx.rollback().await.unwrap();
    assert_eq!(counts(pool).await, before);
    sqlx::raw_sql("CREATE FUNCTION fail_adjustment_guarded() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.operation='OPERATIONAL_ADJUSTMENT_POSTED' THEN RAISE EXCEPTION 'injected adjustment audit failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER fail_adjustment_guarded BEFORE INSERT ON business_core_audit_events FOR EACH ROW EXECUTE FUNCTION fail_adjustment_guarded();").execute(pool).await.unwrap();
    assert!(service
        .post_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "guarded-audit-failure",
            &v,
            &preview
        )
        .await
        .is_err());
    sqlx::raw_sql("DROP TRIGGER fail_adjustment_guarded ON business_core_audit_events; DROP FUNCTION fail_adjustment_guarded();").execute(pool).await.unwrap();
    assert_eq!(
        counts(pool).await,
        before,
        "all writes including allocation previews must roll back"
    );
    let mut bad = preview.clone();
    bad["preview"]["totalAmount"] = json!("999");
    assert!(service
        .post_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "guarded-tampered-amount",
            &v,
            &bad
        )
        .await
        .is_err());
    let order = preview["preview"]["targets"][0]["id"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    sqlx::query(
        "UPDATE sales_orders SET business_note='guarded adjustment version drift' WHERE id=$1",
    )
    .bind(order)
    .execute(pool)
    .await
    .unwrap();
    assert!(service
        .post_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "guarded-stale-target",
            &v,
            &preview
        )
        .await
        .is_err());
    assert_eq!(counts(pool).await, before);
    let preview = service.allocation_preview(f.actor, d.id, &v).await.unwrap();
    let done = service
        .post_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "guarded-final-post",
            &v,
            &preview,
        )
        .await
        .unwrap();
    assert_eq!(done.status, "posted");
    assert_eq!(done.version, 3);
    let sum:Decimal=sqlx::query_scalar("SELECT sum(amount) FROM profit_facts WHERE source_type='operational_adjustment' AND source_id=$1").bind(d.id).fetch_one(pool).await.unwrap();
    assert_eq!(sum, "10.01".parse::<Decimal>().unwrap());
    let after = counts(pool).await;
    let replay = service
        .post_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "guarded-final-post",
            &v,
            &preview,
        )
        .await
        .unwrap();
    assert!(replay.idempotent_replay);
    assert_eq!(replay.id, done.id);
    assert_eq!(counts(pool).await, after);
    assert!(service
        .post_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "guarded-final-post",
            &v,
            &bad
        )
        .await
        .is_err());
    sqlx::query(
        "DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2",
    )
    .bind(f.actor)
    .bind(f.customer)
    .execute(pool)
    .await
    .unwrap();
    assert!(service
        .post_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "guarded-final-post",
            &v,
            &preview
        )
        .await
        .is_err());
    sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.customer).execute(pool).await.unwrap();
    let d = draft(pool, service, f, "guarded-concurrent-draft").await;
    let preview = service.allocation_preview(f.actor, d.id, &v).await.unwrap();
    let (a, b) = tokio::join!(
        service.post_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "guarded-same-key",
            &v,
            &preview
        ),
        service.post_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "guarded-same-key",
            &v,
            &preview
        )
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.id, b.id);
    assert!(a.idempotent_replay || b.idempotent_replay);
    let allocations: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM operational_adjustment_allocations WHERE batch_id=$1",
    )
    .bind(d.id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(allocations, 2);
    wait_revoke(pool, service, f).await;
    late_fact(pool, service, f).await;
    target_wait(pool, service, f).await;
}
async fn wait_revoke(pool: &PgPool, service: &AdjustmentService, f: &Fixture) {
    let d = draft(pool, service, f, "guarded-wait-revoke-draft").await;
    let v = VersionCommand {
        expected_version: 1,
    };
    let preview = service.allocation_preview(f.actor, d.id, &v).await.unwrap();
    let before = counts(pool).await;
    let mut blocker = pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM operational_adjustment_batches WHERE id=$1 FOR UPDATE")
        .bind(d.id)
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    let worker = service.clone();
    let actor = f.actor;
    let task = tokio::spawn(async move {
        worker
            .post_guarded(
                actor,
                Uuid::new_v4(),
                d.id,
                "guarded-wait-revoke",
                &v,
                &preview,
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(pid)
            .fetch_one(pool)
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
    sqlx::query(
        "DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2",
    )
    .bind(f.actor)
    .bind(f.customer)
    .execute(pool)
    .await
    .unwrap();
    blocker.commit().await.unwrap();
    assert!(task.await.unwrap().is_err());
    assert_eq!(counts(pool).await, before);
    sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.customer).execute(pool).await.unwrap();
}

async fn late_fact(pool: &PgPool, service: &AdjustmentService, f: &Fixture) {
    let d = draft(pool, service, f, "guarded-late-fact-draft").await;
    let sql = super::management_snapshot_late_fact::INSERT_FACT.replace("2026-04", "2026-08");
    let mut late = pool.begin().await.unwrap();
    let low: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql.clone()))
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind("7.000000")
        .fetch_one(&mut *late)
        .await
        .unwrap();
    let high: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind("11.000000")
        .fetch_one(pool)
        .await
        .unwrap();
    assert!(low < high);
    let v = VersionCommand {
        expected_version: 1,
    };
    let preview = service.allocation_preview(f.actor, d.id, &v).await.unwrap();
    late.commit().await.unwrap();
    let current = service.allocation_preview(f.actor, d.id, &v).await.unwrap();
    assert_eq!(
        current["preview"]["sourceWatermark"],
        preview["preview"]["sourceWatermark"]
    );
    assert_ne!(
        current["preview"]["allocations"],
        preview["preview"]["allocations"]
    );
    let before = counts(pool).await;
    assert!(service
        .post_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "guarded-late-fact-stale",
            &v,
            &preview
        )
        .await
        .is_err());
    assert_eq!(counts(pool).await, before);
    assert_eq!(
        service
            .post_guarded(
                f.actor,
                Uuid::new_v4(),
                d.id,
                "guarded-late-fact-fresh",
                &v,
                &current
            )
            .await
            .unwrap()
            .status,
        "posted"
    );
}

async fn target_wait(pool: &PgPool, service: &AdjustmentService, f: &Fixture) {
    let d = draft(pool, service, f, "guarded-target-wait-draft").await;
    let v = VersionCommand {
        expected_version: 1,
    };
    let preview = service.allocation_preview(f.actor, d.id, &v).await.unwrap();
    let order = preview["preview"]["targets"][0]["id"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    let before = counts(pool).await;
    let mut blocker = pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM sales_orders WHERE id=$1 FOR UPDATE")
        .bind(order)
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    let worker = service.clone();
    let actor = f.actor;
    let task = tokio::spawn(async move {
        worker
            .post_guarded(
                actor,
                Uuid::new_v4(),
                d.id,
                "guarded-target-wait",
                &v,
                &preview,
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(pid)
            .fetch_one(pool)
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
    sqlx::query(
        "UPDATE sales_orders SET business_note='changed while guarded posting waited' WHERE id=$1",
    )
    .bind(order)
    .execute(&mut *blocker)
    .await
    .unwrap();
    blocker.commit().await.unwrap();
    assert!(task.await.unwrap().is_err());
    assert_eq!(counts(pool).await, before);
}
