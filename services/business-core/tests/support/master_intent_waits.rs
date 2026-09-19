use super::*;
pub async fn check(pool: &PgPool, app: &Router, actor: Uuid, entries: &[(bool, Uuid, Value)]) {
    let (_, customer, fields) = entries
        .iter()
        .find(|(_, _, v)| v["resourceType"] == "customer")
        .unwrap();
    let mut fields = fields.clone();
    fields["expectedVersion"] = json!(3);
    let legal = Uuid::parse_str(fields["legalEntityId"].as_str().unwrap()).unwrap();
    let prepared = prepare(
        app,
        actor,
        "core_master_update_intent",
        json!({"operation":"update","documentId":customer,"command":fields}),
    )
    .await;
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM business_document_approval_votes")
        .fetch_one(pool)
        .await
        .unwrap();
    let mut lock = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM business_legal_entities WHERE id=$1 FOR UPDATE")
        .bind(legal)
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let a = app.clone();
    let path = approval_path("core_master_update_intent", &prepared);
    let command = vote(&prepared);
    let task = tokio::spawn(async move { call(&a, actor, "POST", &path, command, "").await });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(blocker)
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
    sqlx::query("UPDATE business_approval_policies SET step_up_amount_minor=0 WHERE action_code='business_master_data:manage'").execute(pool).await.unwrap();
    lock.commit().await.unwrap();
    let (status, body) = task.await.unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM business_document_approval_votes")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(before, after);
    let version: i64 = sqlx::query_scalar("SELECT version FROM business_customers WHERE id=$1")
        .bind(customer)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(version, 3);
    sqlx::query("UPDATE business_approval_policies SET step_up_amount_minor=NULL WHERE action_code='business_master_data:manage'").execute(pool).await.unwrap();

    // No nested pool checkout is needed while an approval owns the sole connection.
    let single = PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("BUSINESS_CORE_MASTER_INTENT_TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let config = Config::from_env().unwrap();
    let app = business_core::router(AppState::new(PgStore::new(single.clone()), &config));
    tokio::time::timeout(std::time::Duration::from_secs(5),approved(&app,actor,"product_master_creation_intent",json!({"operation":"create","command":{"resourceType":"brand","code":"SINGLE_CONNECTION","name":"One connection"}}))).await.unwrap();
    single.close().await;
}
