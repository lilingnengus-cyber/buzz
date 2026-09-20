use super::*;
pub async fn verify(pool: &PgPool, app: &Router, f: &Fixture, order: Uuid, reviewer: Uuid) {
    let batch = posted(pool, f, order, "reversal-expiry-source").await;
    let prepared = prepare(
        app,
        f.actor,
        REVERSE,
        json!({"batchId":batch,"expectedVersion":3,"reason":"逆转过期测试"}),
    )
    .await;
    let short = Uuid::new_v4();
    sqlx::query("INSERT INTO business_agent_adjustment_intents(id,kind,input,snapshot,created_by_user_id,idempotency_key,trace_id,expires_at) SELECT $1,kind,input,snapshot,created_by_user_id,$1::text,trace_id,clock_timestamp()+interval '2 seconds' FROM business_agent_adjustment_intents WHERE id=$2").bind(short).bind(prepared["item"]["id"].as_str().unwrap().parse::<Uuid>().unwrap()).execute(pool).await.unwrap();
    let mut prepared = prepared;
    prepared["item"]["id"] = json!(short);
    sqlx::raw_sql("CREATE FUNCTION pause_adjustment_vote() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.operation='chat_document_approval_vote' AND NEW.target_type='operational_adjustment_reversal_intent' THEN PERFORM pg_advisory_xact_lock(202609207); END IF; RETURN NEW; END $$; CREATE TRIGGER pause_adjustment_vote BEFORE INSERT ON business_core_audit_events FOR EACH ROW EXECUTE FUNCTION pause_adjustment_vote();").execute(pool).await.unwrap();
    let before = counts(pool).await;
    let mut blocker = pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(202609207)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let app2 = app.clone();
    let path = approval_path(REVERSE, &prepared);
    let command = vote(&prepared);
    let task = tokio::spawn(async move { call(&app2, reviewer, "POST", &path, command, "").await });
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
    sqlx::query("SELECT pg_sleep(GREATEST(EXTRACT(EPOCH FROM (expires_at-clock_timestamp()))::double precision,0)+0.1) FROM business_agent_adjustment_intents WHERE id=$1").bind(short).execute(pool).await.unwrap();
    blocker.commit().await.unwrap();
    let (code, result) = task.await.unwrap();
    assert!(!code.is_success(), "{result}");
    assert_eq!(counts(pool).await, before);
    let status: String =
        sqlx::query_scalar("SELECT status FROM operational_adjustment_batches WHERE id=$1")
            .bind(batch)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(status, "posted");
    assert_eq!(
        call(
            app,
            reviewer,
            "POST",
            &approval_path(REVERSE, &prepared),
            vote(&prepared),
            ""
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    sqlx::raw_sql("DROP TRIGGER pause_adjustment_vote ON business_core_audit_events; DROP FUNCTION pause_adjustment_vote();").execute(pool).await.unwrap();
}
