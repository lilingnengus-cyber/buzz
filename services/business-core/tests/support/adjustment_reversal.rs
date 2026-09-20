use super::{batch, Fixture};
use business_core::b4::{
    model::{CommandResult, VersionCommand},
    AdjustmentService,
};
use chrono::NaiveDate;
use serde_json::{json, Value};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;
async fn tx(pool: &PgPool) -> Transaction<'_, Postgres> {
    let mut t = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *t)
        .await
        .unwrap();
    t
}
async fn state(pool: &PgPool) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('batches',(SELECT jsonb_agg(to_jsonb(b) ORDER BY id) FROM operational_adjustment_batches b),'facts',(SELECT jsonb_agg(to_jsonb(f) ORDER BY id) FROM profit_facts f),'allocations',(SELECT count(*) FROM operational_adjustment_allocations),'events',(SELECT count(*) FROM operational_adjustment_events),'audit',(SELECT count(*) FROM business_core_audit_events),'idem',(SELECT count(*) FROM business_command_idempotency),'outbox',(SELECT count(*) FROM business_core_outbox))").fetch_one(pool).await.unwrap()
}
async fn posted(
    pool: &PgPool,
    service: &AdjustmentService,
    f: &Fixture,
    key: &str,
) -> CommandResult {
    let orders:Vec<Uuid>=sqlx::query_scalar("SELECT sales_order_id FROM order_profit_current WHERE legal_entity_id=$1 AND net_revenue>0 ORDER BY sales_order_id LIMIT 2").bind(f.legal_entity).fetch_all(pool).await.unwrap();
    let input = batch(
        f,
        NaiveDate::from_ymd_opt(2026, 8, 21).unwrap(),
        "allocated_operating_expense",
        "10.01",
        "net_revenue",
        orders,
    );
    let d = service
        .create(f.actor, Uuid::new_v4(), key, &input)
        .await
        .unwrap();
    let input = VersionCommand {
        expected_version: 1,
    };
    let p = service
        .allocation_preview(f.actor, d.id, &input)
        .await
        .unwrap();
    service
        .post_guarded(f.actor, Uuid::new_v4(), d.id, key, &input, &p)
        .await
        .unwrap()
}
pub async fn verify(pool: &PgPool, service: &AdjustmentService, f: &Fixture) {
    let d = posted(pool, service, f, "reverse-bound-source").await;
    let v = VersionCommand {
        expected_version: d.version,
    };
    let reason = "纠正重复录入费用";
    let alternate = Uuid::new_v4();
    sqlx::query("INSERT INTO business_customers(id,legal_entity_id,business_unit_id,code,name,credit_currency,payment_terms_days) VALUES($1,$2,$3,'CUS_REVERSAL_NEW','Current customer','CNY',30)").bind(alternate).bind(f.legal_entity).bind(f.business_unit).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(alternate).execute(pool).await.unwrap();
    let frozen = service
        .reversal_preview(f.actor, d.id, &v, reason)
        .await
        .unwrap();
    sqlx::query("UPDATE sales_orders SET customer_id=$2 WHERE id IN (SELECT sales_order_id FROM operational_adjustment_allocations WHERE batch_id=$1)").bind(d.id).bind(alternate).execute(pool).await.unwrap();
    assert_eq!(
        service
            .reversal_preview(f.actor, d.id, &v, reason)
            .await
            .unwrap(),
        frozen,
        "current order dimensions must not rewrite frozen reversal facts"
    );
    let before = state(pool).await;
    let p = service
        .reversal_preview(f.actor, d.id, &v, reason)
        .await
        .unwrap();
    assert_eq!(
        service
            .reversal_preview(f.actor, d.id, &v, reason)
            .await
            .unwrap(),
        p
    );
    assert_eq!(state(pool).await, before);
    assert_eq!(p["preview"]["facts"].as_array().unwrap().len(), 2);
    assert_eq!(p["preview"]["totalAmount"], "10.010000");
    assert!(service
        .reversal_preview(f.actor, d.id, &v, "")
        .await
        .is_err());
    assert!(service
        .reversal_preview(
            f.actor,
            d.id,
            &VersionCommand {
                expected_version: 99
            },
            reason
        )
        .await
        .is_err());
    let mut malformed = p.clone();
    malformed["preview"]["facts"][0]["fact"]["amount"] = json!("999");
    assert!(service
        .reverse_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "reverse-tampered",
            &v,
            reason,
            &malformed
        )
        .await
        .is_err());
    assert!(service
        .reverse_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "reverse-reason-changed",
            &v,
            "different",
            &p
        )
        .await
        .is_err());
    assert_eq!(state(pool).await, before);
    let mut t = pool.begin().await.unwrap();
    assert!(service
        .reverse_guarded_on(
            &mut t,
            f.actor,
            Uuid::new_v4(),
            d.id,
            "reverse-low-isolation",
            &v,
            reason,
            &p
        )
        .await
        .is_err());
    t.rollback().await.unwrap();
    let mut t = tx(pool).await;
    let result = service
        .reverse_guarded_on(
            &mut t,
            f.actor,
            Uuid::new_v4(),
            d.id,
            "reverse-rollback",
            &v,
            reason,
            &p,
        )
        .await
        .unwrap();
    assert_eq!(result.status, "reversed");
    t.rollback().await.unwrap();
    assert_eq!(state(pool).await, before);
    sqlx::raw_sql("CREATE FUNCTION fail_bound_reversal() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.operation='OPERATIONAL_ADJUSTMENT_REVERSED' THEN RAISE EXCEPTION 'injected reversal audit failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER fail_bound_reversal BEFORE INSERT ON business_core_audit_events FOR EACH ROW EXECUTE FUNCTION fail_bound_reversal();").execute(pool).await.unwrap();
    assert!(service
        .reverse_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "reverse-audit-failure",
            &v,
            reason,
            &p
        )
        .await
        .is_err());
    assert_eq!(state(pool).await, before);
    sqlx::raw_sql("DROP TRIGGER fail_bound_reversal ON business_core_audit_events; DROP FUNCTION fail_bound_reversal();").execute(pool).await.unwrap();
    let result = service
        .reverse_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "reverse-execute",
            &v,
            reason,
            &p,
        )
        .await
        .unwrap();
    assert_eq!(result.version, d.version + 1);
    let after = state(pool).await;
    assert!(service
        .reverse_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "reverse-execute",
            &v,
            "changed reason",
            &p
        )
        .await
        .is_err());

    assert!(
        service
            .reverse_guarded(
                f.actor,
                Uuid::new_v4(),
                d.id,
                "reverse-execute",
                &v,
                reason,
                &p
            )
            .await
            .unwrap()
            .idempotent_replay
    );
    assert_eq!(state(pool).await, after);
    // Every reversing fact must copy frozen amount, period and dimensions; never today's values.
    let exact:bool=sqlx::query_scalar("SELECT count(*)=2 AND bool_and((to_jsonb(r)-ARRAY['id','direction','fact_sequence','source_event_id','source_event_version','data_as_of','trace_id','created_at'])=(to_jsonb(n)-ARRAY['id','direction','fact_sequence','source_event_id','source_event_version','data_as_of','trace_id','created_at'])) FROM profit_facts n JOIN profit_facts r ON r.source_line_id=n.source_line_id AND r.direction='reversal' WHERE n.source_id=$1 AND n.direction='normal'").bind(d.id).fetch_one(pool).await.unwrap();
    assert!(exact);
    let audit:Value=sqlx::query_scalar("SELECT details FROM business_core_audit_events WHERE target_id=$1 AND operation='OPERATIONAL_ADJUSTMENT_REVERSED'").bind(d.id.to_string()).fetch_one(pool).await.unwrap();
    assert_eq!(audit["reason"], reason);
    let event:Value=sqlx::query_scalar("SELECT payload FROM operational_adjustment_events WHERE batch_id=$1 AND event_type='reversed'").bind(d.id).fetch_one(pool).await.unwrap();
    assert_eq!(event["reason"], reason);
    sqlx::query(
        "DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2",
    )
    .bind(f.actor)
    .bind(f.customer)
    .execute(pool)
    .await
    .unwrap();
    assert!(service
        .reverse_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "reverse-execute",
            &v,
            reason,
            &p
        )
        .await
        .is_err());
    sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.customer).execute(pool).await.unwrap();
    wait_revoke(pool, service, f).await;
    let d = posted(pool, service, f, "reverse-concurrent-source").await;
    let v = VersionCommand {
        expected_version: d.version,
    };
    let p = service
        .reversal_preview(f.actor, d.id, &v, reason)
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        service.reverse_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "reverse-concurrent",
            &v,
            reason,
            &p
        ),
        service.reverse_guarded(
            f.actor,
            Uuid::new_v4(),
            d.id,
            "reverse-concurrent",
            &v,
            reason,
            &p
        )
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_ne!(a.idempotent_replay, b.idempotent_replay);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM profit_facts WHERE source_id=$1 AND direction='reversal'",
    )
    .bind(d.id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(count, 2);
}
async fn wait_revoke(pool: &PgPool, service: &AdjustmentService, f: &Fixture) {
    let d = posted(pool, service, f, "reverse-wait-source").await;
    let v = VersionCommand {
        expected_version: d.version,
    };
    let p = service
        .reversal_preview(f.actor, d.id, &v, "撤权测试")
        .await
        .unwrap();
    let before = state(pool).await;
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
            .reverse_guarded(
                actor,
                Uuid::new_v4(),
                d.id,
                "reverse-wait-revoke",
                &v,
                "撤权测试",
                &p,
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
    assert_eq!(state(pool).await, before);
    sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.customer).execute(pool).await.unwrap();
}
