use super::*;
async fn brand(app: &Router, actor: Uuid) -> Value {
    prepare(app,actor,"product_master_creation_intent",json!({"operation":"create","command":{"resourceType":"brand","code":Uuid::new_v4().simple().to_string().to_uppercase(),"name":"IAM brand"}})).await
}
async fn approve(app: &Router, actor: Uuid, prepared: &Value) -> (StatusCode, Value) {
    call(
        app,
        actor,
        "POST",
        &approval_path("product_master_creation_intent", prepared),
        vote(prepared),
        "",
    )
    .await
}
async fn totals(pool: &PgPool) -> (i64, i64, i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM business_brands),(SELECT count(*) FROM business_brand_scopes),(SELECT count(*) FROM business_document_approval_requests),(SELECT count(*) FROM business_document_approval_votes),(SELECT count(*) FROM business_core_audit_events)").fetch_one(pool).await.unwrap()
}
pub async fn check(pool: &PgPool, app: &Router) {
    let actor = Uuid::new_v4();
    let core_role = Uuid::new_v4();
    let principal = Uuid::new_v4();
    let iam_role = Uuid::new_v4();
    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'master-intents',$1::text,'IAM approver')").bind(actor).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_roles(id,role_key,name) VALUES($1,'master_iam_approver','IAM approver')").bind(core_role).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) VALUES($1,$2,$1)",
    )
    .bind(actor)
    .bind(core_role)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_iam.principals(id,kind,external_id,display_name) VALUES($1,'human',$2,'IAM approver')").bind(principal).bind(actor.to_string()).execute(pool).await.unwrap();
    let permission:Uuid=sqlx::query_scalar("INSERT INTO business_iam.permissions(id,capability,resource_type,action) VALUES(gen_random_uuid(),'business_product_master:manage','business_product_master','manage') ON CONFLICT(capability) DO UPDATE SET capability=excluded.capability RETURNING id").fetch_one(pool).await.unwrap();
    sqlx::query("UPDATE business_approval_policies SET eligible_role_keys=ARRAY['master_intent_approver','master_iam_approver'] WHERE action_code='business_product_master:manage'").execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_iam.principal_permissions(principal_id,permission_id) VALUES($1,$2)",
    )
    .bind(principal)
    .bind(permission)
    .execute(pool)
    .await
    .unwrap();
    let prepared = brand(app, actor).await;
    let (status, result) = approve(app, actor, &prepared).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true);
    sqlx::query("DELETE FROM business_iam.principal_permissions WHERE principal_id=$1")
        .bind(principal)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO business_iam.roles(id,code,name) VALUES($1,'master_intent_iam','IAM master')",
    )
    .bind(iam_role)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO business_iam.role_permissions(role_id,permission_id) VALUES($1,$2)")
        .bind(iam_role)
        .bind(permission)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO business_iam.principal_roles(principal_id,role_id) VALUES($1,$2)")
        .bind(principal)
        .bind(iam_role)
        .execute(pool)
        .await
        .unwrap();
    let prepared = brand(app, actor).await;
    let (status, result) = approve(app, actor, &prepared).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true);

    // Expiry happens after the business insert, during the final approval update.
    // A final authority deadline check must roll back the entire transaction.
    sqlx::query("CREATE FUNCTION master_test_wait_execution() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.status='executed' THEN PERFORM pg_advisory_xact_lock(80552026); END IF; RETURN NEW; END $$").execute(pool).await.unwrap();
    sqlx::query("CREATE TRIGGER master_test_wait_execution BEFORE UPDATE ON business_document_approval_requests FOR EACH ROW EXECUTE FUNCTION master_test_wait_execution()").execute(pool).await.unwrap();
    let mut lock = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(80552026)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let deadline:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("UPDATE business_iam.principal_roles SET valid_until=clock_timestamp()+interval '3 seconds' WHERE principal_id=$1 RETURNING valid_until").bind(principal).fetch_one(pool).await.unwrap();
    let prepared = brand(app, actor).await;
    let before = totals(pool).await;
    let a = app.clone();
    let task = tokio::spawn(async move { approve(&a, actor, &prepared).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
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
    sqlx::query(
        "SELECT pg_sleep(GREATEST(0,extract(epoch FROM ($1::timestamptz-clock_timestamp()))+0.05))",
    )
    .bind(deadline)
    .execute(pool)
    .await
    .unwrap();
    lock.commit().await.unwrap();
    let (status, result) = task.await.unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND, "{result}");
    assert_eq!(totals(pool).await, before);
    sqlx::query("DROP TRIGGER master_test_wait_execution ON business_document_approval_requests")
        .execute(pool)
        .await
        .unwrap();
}
