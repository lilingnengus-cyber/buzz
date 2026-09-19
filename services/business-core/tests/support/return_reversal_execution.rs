use super::*;
use sqlx::Row;

pub(super) async fn check(
    app: &Router,
    store: &PgStore,
    f: &Fixture,
    sales: bool,
    id: Uuid,
    version: i64,
) {
    let service = business_core::b2::ReturnService::new(store.clone(), "SR".into(), "PR".into());
    let input = business_core::b2::ReverseReturn {
        expected_version: version,
        reversal_date: NaiveDate::from_ymd_opt(2026, 10, 21).unwrap(),
        reason: "登记错误，保留原流水".into(),
    };
    let preview = service
        .reversal_preview(f.actor, sales, id, &input)
        .await
        .unwrap();
    let kind = if sales {
        "sales_return"
    } else {
        "purchase_return"
    };
    let financial_table = if sales {
        "trade_receivables"
    } else {
        "trade_payables"
    };
    let financial_id = preview["financial"]["id"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    let mut blocker = store.pool().begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT id FROM {financial_table} WHERE id=$1 FOR UPDATE"
    )))
    .bind(financial_id)
    .fetch_one(&mut *blocker)
    .await
    .unwrap();
    let concurrent = service.clone();
    let actor = f.actor;
    let pending_input = input.clone();
    let pending_preview = preview.clone();
    let waiting = tokio::spawn(async move {
        concurrent
            .reverse_return_guarded(
                (actor, Uuid::new_v4()),
                sales,
                id,
                "racing-reversal",
                &pending_input,
                &pending_preview,
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10),async { loop {
        let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)))").bind(pid).fetch_one(store.pool()).await.unwrap();
        if blocked { break; }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }}).await.expect("reversal is waiting on the financial row lock");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE {financial_table} SET trace_id=$2 WHERE id=$1"
    )))
    .bind(financial_id)
    .bind(Uuid::new_v4())
    .execute(&mut *blocker)
    .await
    .unwrap();
    blocker.commit().await.unwrap();
    assert!(matches!(
        waiting.await.unwrap(),
        Err(DomainError::VersionConflict)
    ));
    assert!(matches!(
        service
            .reverse_return_guarded(
                (f.actor, Uuid::new_v4()),
                sales,
                id,
                "stale-reversal",
                &input,
                &preview
            )
            .await,
        Err(DomainError::VersionConflict)
    ));
    let approved = service
        .reversal_preview(f.actor, sales, id, &input)
        .await
        .unwrap();
    let originals:Value=sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(m) ORDER BY m.id) FROM inventory_movements m WHERE source_type=$1 AND source_id=$2").bind(kind).bind(id).fetch_one(store.pool()).await.unwrap();
    // Force failure after inventory and financial writes; the complete command must roll back.
    sqlx::query("CREATE OR REPLACE FUNCTION test_return_reversal_failure() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_type='reversed' THEN RAISE EXCEPTION 'test reversal rollback'; END IF; RETURN NEW; END $$").execute(store.pool()).await.unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE TRIGGER test_return_reversal_failure BEFORE INSERT ON {kind}_events FOR EACH ROW EXECUTE FUNCTION test_return_reversal_failure()"))).execute(store.pool()).await.unwrap();
    assert!(service
        .reverse_return_guarded(
            (f.actor, Uuid::new_v4()),
            sales,
            id,
            &format!("atomic-reversal-{id}"),
            &input,
            &approved
        )
        .await
        .is_err());
    assert_eq!(
        service
            .reversal_preview(f.actor, sales, id, &input)
            .await
            .unwrap(),
        approved
    );
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER test_return_reversal_failure ON {kind}_events"
    )))
    .execute(store.pool())
    .await
    .unwrap();
    if sales && version == 3 {
        business_core::b4::ProfitProjectionService::new(store.clone())
            .project_pending(f.actor, Uuid::new_v4(), 1000)
            .await
            .unwrap();
    }
    let metrics_before = period_metrics(store, f, sales).await;
    let execution_key = return_disposition_checks::execute(
        app,
        store,
        f,
        id,
        &format!("{kind}_reversal_intent"),
        serde_json::to_value(&input).unwrap(),
    )
    .await;
    let result = service
        .reverse_return_guarded(
            (f.actor, Uuid::new_v4()),
            sales,
            id,
            &execution_key,
            &input,
            &approved,
        )
        .await
        .unwrap();
    assert_eq!(result.status, "reversed");
    let metrics_after = period_metrics(store, f, sales).await;
    assert_eq!(
        metrics_after.0, metrics_before.0,
        "original month count remains recorded"
    );
    assert_eq!(
        metrics_after.1, metrics_before.1,
        "original month amount remains recorded"
    );
    assert_eq!(
        metrics_after.2,
        metrics_before.2 - 1,
        "correction month records the net count reduction"
    );
    assert_eq!(metrics_after.3, metrics_before.3 - Decimal::from(100));

    assert_eq!(result.version, version + 1);
    assert!(
        service
            .reverse_return_guarded(
                (f.actor, Uuid::new_v4()),
                sales,
                id,
                &execution_key,
                &input,
                &approved
            )
            .await
            .unwrap()
            .idempotent_replay
    );
    assert!(service
        .reverse_return_guarded(
            (f.actor, Uuid::new_v4()),
            sales,
            id,
            "second-reversal",
            &input,
            &approved
        )
        .await
        .is_err());
    let unchanged:Value=sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(m) ORDER BY m.id) FROM inventory_movements m WHERE source_type=$1 AND source_id=$2").bind(kind).bind(id).fetch_one(store.pool()).await.unwrap();
    assert_eq!(originals, unchanged);
    let inverse_count:i64=sqlx::query_scalar("SELECT count(*) FROM inventory_movements m JOIN inventory_movements o ON o.id=m.reversal_of_movement_id WHERE m.source_type=$1 AND m.source_id=$2 AND m.quantity=-o.quantity AND m.total_cost=-o.total_cost AND m.posting_sequence>o.posting_sequence").bind(format!("{kind}_reversal")).bind(id).fetch_one(store.pool()).await.unwrap();
    assert_eq!(
        inverse_count,
        approved["inverseMovements"].as_array().unwrap().len() as i64
    );
    for effect in approved["lines"].as_array().unwrap() {
        let balance=sqlx::query("SELECT on_hand_quantity,quarantined_quantity,inventory_value,average_unit_cost FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3").bind(f.legal_entity).bind(f.warehouse).bind(effect["skuId"].as_str().unwrap().parse::<Uuid>().unwrap()).fetch_one(store.pool()).await.unwrap();
        for (column, key) in [
            ("on_hand_quantity", "onHandQuantityAfter"),
            ("quarantined_quantity", "quarantinedQuantityAfter"),
            ("inventory_value", "inventoryValueAfter"),
        ] {
            assert_eq!(
                balance.get::<Decimal, _>(column),
                effect[key].as_str().unwrap().parse::<Decimal>().unwrap()
            );
        }
    }
    let financial=sqlx::query(sqlx::AssertSqlSafe(format!("SELECT original_amount,open_amount,settled_amount,version FROM {financial_table} WHERE id=$1"))).bind(financial_id).fetch_one(store.pool()).await.unwrap();
    for (column, key) in [
        ("original_amount", "originalAmountAfter"),
        ("open_amount", "openAmountAfter"),
        ("settled_amount", "settledAmount"),
    ] {
        assert_eq!(
            financial.get::<Decimal, _>(column),
            approved["financial"][key]
                .as_str()
                .unwrap()
                .parse::<Decimal>()
                .unwrap()
        );
    }
    assert_eq!(
        financial.get::<i64, _>("version"),
        approved["financial"]["version"].as_i64().unwrap() + 1
    );
    let event: Value = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT payload FROM {kind}_events WHERE {kind}_id=$1 AND event_type='reversed'"
    )))
    .bind(id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(event["reason"], input.reason);
    assert_eq!(event["effects"], approved);
    if sales {
        check_projection(store, f, id, version).await;
    }
    sqlx::query("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2")
        .bind(f.actor)
        .bind(f.brand)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(matches!(
        service
            .reverse_return_guarded(
                (f.actor, Uuid::new_v4()),
                sales,
                id,
                &execution_key,
                &input,
                &approved
            )
            .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.brand).execute(store.pool()).await.unwrap();
}

async fn check_projection(store: &PgStore, f: &Fixture, id: Uuid, version: i64) {
    let service = business_core::b4::ProfitProjectionService::new(store.clone());
    let changed_master = if version == 3 {
        let row=sqlx::query("SELECT p.id,p.category_id FROM sales_return_lines l JOIN business_skus sku ON sku.id=l.sku_id JOIN business_products p ON p.id=sku.product_id WHERE l.sales_return_id=$1 LIMIT 1").bind(id).fetch_one(store.pool()).await.unwrap();
        let product: Uuid = row.get("id");
        let old_category: Uuid = row.get("category_id");
        let category = Uuid::new_v4();
        sqlx::query("INSERT INTO business_product_categories(id,code,name) VALUES($1,$2,'Changed after original projection')").bind(category).bind(format!("CHANGED-{category}")).execute(store.pool()).await.unwrap();
        sqlx::query("UPDATE business_products SET category_id=$2 WHERE id=$1")
            .bind(product)
            .bind(category)
            .execute(store.pool())
            .await
            .unwrap();
        Some((product, old_category))
    } else {
        None
    };
    if version == 2 {
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE OR REPLACE FUNCTION test_return_fact_failure() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.source_id='{id}'::uuid AND NEW.direction='reversal' AND NEW.metric_type='product_cost' THEN RAISE EXCEPTION 'test partial projection rollback'; END IF; RETURN NEW; END $$"))).execute(store.pool()).await.unwrap();
        sqlx::query("CREATE TRIGGER test_return_fact_failure BEFORE INSERT ON profit_facts FOR EACH ROW EXECUTE FUNCTION test_return_fact_failure()").execute(store.pool()).await.unwrap();
        let outcome = service
            .project_pending(f.actor, Uuid::new_v4(), 1000)
            .await
            .unwrap();
        assert!(outcome["failures"].as_i64().unwrap() > 0);
        let partial: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM profit_facts WHERE source_id=$1 AND direction='normal'",
        )
        .bind(id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(
            partial, 0,
            "first metric must roll back when the second metric fails"
        );
        let pending:i64=sqlx::query_scalar("SELECT count(*) FROM profit_projection_failures WHERE aggregate_id=$1 AND status='pending'").bind(id).fetch_one(store.pool()).await.unwrap();
        assert_eq!(
            pending, 2,
            "database failures remain retryable without aborting the batch"
        );
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE OR REPLACE FUNCTION test_return_fact_failure() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.source_id='{id}'::uuid AND NEW.direction='normal' AND NEW.metric_type='product_cost' THEN RAISE EXCEPTION 'test correction rollback'; END IF; RETURN NEW; END $$"))).execute(store.pool()).await.unwrap();
        let retried = service
            .project_pending(f.actor, Uuid::new_v4(), 1000)
            .await
            .unwrap();
        assert_eq!(retried["failures"], 1);
        let original_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM profit_facts WHERE source_id=$1 AND direction='reversal'",
        )
        .bind(id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(original_count, 2);
        let correction_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM profit_facts WHERE source_id=$1 AND direction='normal'",
        )
        .bind(id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(
            correction_count, 0,
            "correction copy is atomic after original facts become available"
        );
        sqlx::query("DROP TRIGGER test_return_fact_failure ON profit_facts")
            .execute(store.pool())
            .await
            .unwrap();
    }

    service
        .project_pending(f.actor, Uuid::new_v4(), 1000)
        .await
        .unwrap();
    let failures:i64=sqlx::query_scalar("SELECT count(*) FROM profit_projection_failures WHERE aggregate_id=$1 AND status='pending'").bind(id).fetch_one(store.pool()).await.unwrap();
    assert_eq!(failures, 0);
    let facts:Value=sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(f) ORDER BY fact_sequence) FROM profit_facts f WHERE source_type='sales_return' AND source_id=$1").bind(id).fetch_one(store.pool()).await.unwrap();
    let rows = facts.as_array().unwrap();
    assert_eq!(
        rows.len(),
        4,
        "confirmation and correction each produce two facts"
    );
    for row in rows {
        let correction = row["direction"] == "normal";
        assert_eq!(
            row["business_date"],
            if correction {
                "2026-10-21"
            } else {
                "2026-09-19"
            }
        );
        assert_eq!(
            row["management_period"],
            if correction { "2026-10" } else { "2026-09" }
        );
        assert_eq!(
            row["source_event_version"],
            if correction { version + 1 } else { 2 }
        );
    }
    for restored in rows.iter().filter(|row| row["direction"] == "normal") {
        let original = rows
            .iter()
            .find(|row| {
                row["direction"] == "reversal"
                    && row["metric_type"] == restored["metric_type"]
                    && row["source_line_id"] == restored["source_line_id"]
            })
            .unwrap();
        for key in [
            "amount",
            "currency",
            "quantity",
            "legal_entity_id",
            "sales_order_id",
            "sales_order_line_id",
            "shipment_id",
            "shipment_line_id",
            "customer_id",
            "sku_id",
            "product_category_id",
            "brand_id",
            "salesperson_user_id",
            "business_unit_id",
            "department_id",
            "warehouse_id",
        ] {
            assert_eq!(
                restored[key], original[key],
                "correction preserves original {key}"
            );
        }
    }
    let mismatch:i64=sqlx::query_scalar("SELECT count(*) FROM profit_projection_reconciliation WHERE shipment_line_id IN (SELECT shipment_line_id FROM sales_return_lines WHERE sales_return_id=$1) AND (revenue_difference<>0 OR cost_difference<>0)").bind(id).fetch_one(store.pool()).await.unwrap();
    assert_eq!(mismatch, 0);
    service.rebuild(f.actor, Uuid::new_v4()).await.unwrap();
    let again:Value=sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(f) ORDER BY fact_sequence) FROM profit_facts f WHERE source_type='sales_return' AND source_id=$1").bind(id).fetch_one(store.pool()).await.unwrap();
    assert_eq!(
        facts, again,
        "rebuild must neither duplicate nor rewrite original facts"
    );
    if let Some((product, category)) = changed_master {
        sqlx::query("UPDATE business_products SET category_id=$2 WHERE id=$1")
            .bind(product)
            .bind(category)
            .execute(store.pool())
            .await
            .unwrap();
    }
}

async fn period_metrics(store: &PgStore, f: &Fixture, sales: bool) -> (i64, Decimal, i64, Decimal) {
    let (count, amount) = if sales {
        ("sales_return_count", "sales_return_amount")
    } else {
        ("purchase_return_count", "purchase_return_amount")
    };
    sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT COALESCE(sum({count}) FILTER(WHERE management_period='2026-09-01'),0)::bigint,COALESCE(sum({amount}) FILTER(WHERE management_period='2026-09-01'),0),COALESCE(sum({count}) FILTER(WHERE management_period='2026-10-01'),0)::bigint,COALESCE(sum({amount}) FILTER(WHERE management_period='2026-10-01'),0) FROM return_operating_metrics WHERE legal_entity_id=$1 AND currency='CNY'"))).bind(f.legal_entity).fetch_one(store.pool()).await.unwrap()
}
