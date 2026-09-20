use business_core::{
    b4::{
        model::{CreateAdjustmentBatch, ReplaceAdjustmentDraft},
        AdjustmentService,
    },
    PgStore,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool, Postgres, Transaction};
use uuid::Uuid;
#[path = "support/b2_seed.rs"]
mod b2_seed;
#[path = "support/adjustment_intent_fixture.rs"]
mod fixture;
#[path = "support/adjustment_draft_preview.rs"]
mod preview;
struct Fixture {
    actor: Uuid,
    legal_entity: Uuid,
    business_unit: Uuid,
    warehouse: Uuid,
    customer: Uuid,
    brand: Uuid,
    uom: Uuid,
    sku: Uuid,
}
async fn tx(pool: &PgPool) -> Transaction<'_, Postgres> {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx
}
async fn counts(pool: &PgPool) -> Vec<i64> {
    sqlx::query_scalar("SELECT n FROM (SELECT count(*) n FROM operational_adjustment_batches UNION ALL SELECT count(*) FROM operational_adjustment_lines UNION ALL SELECT count(*) FROM operational_adjustment_events UNION ALL SELECT count(*) FROM business_core_audit_events UNION ALL SELECT count(*) FROM business_command_idempotency UNION ALL SELECT count(*) FROM profit_facts UNION ALL SELECT count(*) FROM business_core_outbox UNION ALL SELECT count(*) FROM business_numbering_issuances UNION ALL SELECT COALESCE(sum(current_value),0)::bigint FROM business_numbering_sequence_pools) counts").fetch_all(pool).await.unwrap()
}
#[tokio::test]
async fn draft_transactions_are_atomic_scoped_versioned_and_idempotent() {
    let Ok(url) = std::env::var("BUSINESS_CORE_ADJUSTMENT_DRAFT_TEST_DATABASE_URL") else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect(&url)
        .await
        .unwrap();
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let f = b2_seed::seed(&pool).await;
    let order = fixture::source(&pool, &f).await;
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,'profit_adjustment:update_draft' FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).execute(&pool).await.unwrap();
    let service = AdjustmentService::new(store, "ADJ".into(), 500);
    let input:CreateAdjustmentBatch=serde_json::from_value(json!({"legalEntityId":f.legal_entity,"currency":"CNY","managementPeriod":"2026-08","lines":[{"metricType":"allocated_operating_expense","amount":"10.01","businessDate":"2026-08-21","allocationBasis":"direct","directSalesOrderId":order,"reasonCode":"TEST"}]})).unwrap();
    preview::verify(&pool, &service, &f, order, &input).await;
    let before = counts(&pool).await;
    let mut t = pool.begin().await.unwrap();
    assert!(service
        .create_guarded_on(&mut t, f.actor, Uuid::new_v4(), "invalid-isolation", &input)
        .await
        .is_err());
    t.rollback().await.unwrap();
    let mut t = tx(&pool).await;
    service
        .create_guarded_on(
            &mut t,
            f.actor,
            Uuid::new_v4(),
            "rolled-back-create",
            &input,
        )
        .await
        .unwrap();
    t.rollback().await.unwrap();
    assert_eq!(counts(&pool).await, before);
    let mut t = tx(&pool).await;
    let created = service
        .create_guarded_on(&mut t, f.actor, Uuid::new_v4(), "guarded-created", &input)
        .await
        .unwrap();
    t.commit().await.unwrap();
    let before = counts(&pool).await;
    let mut t = tx(&pool).await;
    let replay = service
        .create_guarded_on(&mut t, f.actor, Uuid::new_v4(), "guarded-created", &input)
        .await
        .unwrap();
    assert_eq!(replay.id, created.id);
    assert!(replay.idempotent_replay);
    t.commit().await.unwrap();
    assert_eq!(counts(&pool).await, before);
    let mut replacement = ReplaceAdjustmentDraft {
        expected_version: 1,
        batch: input.clone(),
    };
    replacement.batch.lines[0].amount.0 = "20.02".parse().unwrap();
    let mut t = tx(&pool).await;
    let updated = service
        .replace_draft_guarded_on(
            &mut t,
            f.actor,
            Uuid::new_v4(),
            created.id,
            "guarded-replace",
            &replacement,
        )
        .await
        .unwrap();
    assert_eq!(updated.version, 2);
    t.rollback().await.unwrap();
    assert_eq!(counts(&pool).await, before);
    let current:(i64,String)=sqlx::query_as("SELECT b.version,l.amount::text FROM operational_adjustment_batches b JOIN operational_adjustment_lines l ON l.batch_id=b.id WHERE b.id=$1").bind(created.id).fetch_one(&pool).await.unwrap();
    assert_eq!(current, (1, "10.010000".into()));
    let mut t = tx(&pool).await;
    service
        .replace_draft_guarded_on(
            &mut t,
            f.actor,
            Uuid::new_v4(),
            created.id,
            "guarded-replace",
            &replacement,
        )
        .await
        .unwrap();
    t.commit().await.unwrap();
    let before = counts(&pool).await;
    let mut t = tx(&pool).await;
    assert!(
        service
            .replace_draft_guarded_on(
                &mut t,
                f.actor,
                Uuid::new_v4(),
                created.id,
                "guarded-replace",
                &replacement
            )
            .await
            .unwrap()
            .idempotent_replay
    );
    t.commit().await.unwrap();
    assert_eq!(counts(&pool).await, before);
    let mut t = tx(&pool).await;
    assert!(service
        .replace_draft_guarded_on(
            &mut t,
            f.actor,
            Uuid::new_v4(),
            created.id,
            "guarded-stale",
            &replacement
        )
        .await
        .is_err());
    t.rollback().await.unwrap();
    // Same key and payload cannot replay a different batch's mutation.
    let other = fixture::draft(&pool, &f, order, "other-draft").await;
    let before = counts(&pool).await;
    let mut t = tx(&pool).await;
    assert!(service
        .replace_draft_guarded_on(
            &mut t,
            f.actor,
            Uuid::new_v4(),
            other,
            "guarded-replace",
            &replacement
        )
        .await
        .is_err());
    t.rollback().await.unwrap();
    assert_eq!(counts(&pool).await, before);
    // Every explicit reference is checked, even if unused by the selected allocation basis.
    let mut hidden = input.clone();
    hidden.lines[0].sales_order_ids.push(Uuid::new_v4());
    let mut t = tx(&pool).await;
    assert!(service
        .create_guarded_on(&mut t, f.actor, Uuid::new_v4(), "guarded-hidden", &hidden)
        .await
        .is_err());
    t.rollback().await.unwrap();
    assert_eq!(counts(&pool).await, before);
    wait_revoke(&pool, &service, &f, order, created.id, &input).await;
    sqlx::raw_sql("CREATE FUNCTION fail_guarded_draft() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.operation IN ('OPERATIONAL_ADJUSTMENT_CREATED','OPERATIONAL_ADJUSTMENT_UPDATED') THEN RAISE EXCEPTION 'injected draft audit failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER fail_guarded_draft BEFORE INSERT ON business_core_audit_events FOR EACH ROW EXECUTE FUNCTION fail_guarded_draft();").execute(&pool).await.unwrap();
    for replace in [false, true] {
        let mut t = tx(&pool).await;
        let mut update = replacement.clone();
        update.expected_version = 2;
        let result = if replace {
            service
                .replace_draft_guarded_on(
                    &mut t,
                    f.actor,
                    Uuid::new_v4(),
                    created.id,
                    "audit-replace",
                    &update,
                )
                .await
        } else {
            service
                .create_guarded_on(&mut t, f.actor, Uuid::new_v4(), "audit-create", &input)
                .await
        };
        assert!(result.is_err());
        t.rollback().await.unwrap();
        assert_eq!(counts(&pool).await, before);
    }
    sqlx::raw_sql("DROP TRIGGER fail_guarded_draft ON business_core_audit_events; DROP FUNCTION fail_guarded_draft();").execute(&pool).await.unwrap();
    // Removing a restricted old line through a replacement cannot bypass whole-draft access.
    sqlx::query(
        "DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2",
    )
    .bind(f.actor)
    .bind(f.customer)
    .execute(&pool)
    .await
    .unwrap();
    let mut clean = replacement.clone();
    clean.expected_version = 2;
    clean.batch.lines[0].direct_sales_order_id = None;
    clean.batch.lines[0].allocation_basis = "net_revenue".into();
    for replay in [false, true] {
        let mut t = tx(&pool).await;
        assert!(service
            .replace_draft_guarded_on(
                &mut t,
                f.actor,
                Uuid::new_v4(),
                created.id,
                if replay {
                    "guarded-replace"
                } else {
                    "remove-hidden"
                },
                if replay { &replacement } else { &clean }
            )
            .await
            .is_err());
        t.rollback().await.unwrap();
    }
    let mut t = tx(&pool).await;
    assert!(service
        .create_guarded_on(&mut t, f.actor, Uuid::new_v4(), "guarded-created", &input)
        .await
        .is_err());
    t.rollback().await.unwrap();
    assert_eq!(counts(&pool).await, before);
}

async fn wait_revoke(
    pool: &PgPool,
    service: &AdjustmentService,
    f: &Fixture,
    order: Uuid,
    batch: Uuid,
    input: &CreateAdjustmentBatch,
) {
    for replace in [false, true] {
        let before = counts(pool).await;
        let mut blocker = pool.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *blocker)
            .await
            .unwrap();
        let query = if replace {
            "SELECT id FROM operational_adjustment_batches WHERE id=$1 FOR UPDATE"
        } else {
            "SELECT id FROM sales_orders WHERE id=$1 FOR UPDATE"
        };
        sqlx::query(sqlx::AssertSqlSafe(query))
            .bind(if replace { batch } else { order })
            .fetch_one(&mut *blocker)
            .await
            .unwrap();
        let worker = service.clone();
        let worker_pool = pool.clone();
        let actor = f.actor;
        let input = input.clone();
        let task = tokio::spawn(async move {
            let mut t = tx(&worker_pool).await;
            let result = if replace {
                worker
                    .replace_draft_guarded_on(
                        &mut t,
                        actor,
                        Uuid::new_v4(),
                        batch,
                        "wait-replace",
                        &ReplaceAdjustmentDraft {
                            expected_version: 2,
                            batch: input,
                        },
                    )
                    .await
            } else {
                worker
                    .create_guarded_on(&mut t, actor, Uuid::new_v4(), "wait-create", &input)
                    .await
            };
            t.rollback().await.unwrap();
            result
        });
        tokio::time::timeout(std::time::Duration::from_secs(5),async {
            loop {
                let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))").bind(pid).fetch_one(pool).await.unwrap();
                if waiting {break;}
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.unwrap();
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
}
