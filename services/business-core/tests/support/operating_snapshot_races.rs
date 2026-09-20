use super::*;
const KIND: &str = "operating_report_snapshot_intent";

async fn blocked(pool: &PgPool, n: i64) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND cardinality(pg_blocking_pids(pid))>0")
                .fetch_one(pool).await.unwrap();
            if count >= n { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
}
fn spawn(
    app: &Router,
    actor: Uuid,
    prepared: &Value,
) -> tokio::task::JoinHandle<(StatusCode, Value)> {
    let app = app.clone();
    let prepared = prepared.clone();
    tokio::spawn(async move {
        call(
            &app,
            actor,
            "POST",
            &approval_path(KIND, &prepared),
            vote(&prepared),
            "",
        )
        .await
    })
}
fn input(cadence: &str, month: &str) -> Value {
    let day = match month {
        "2026-07" => "2026-07-06",
        "2026-06" => "2026-06-01",
        "2026-05" => "2026-05-04",
        "2026-04" => "2026-04-06",
        _ => "2026-03-02",
    };
    json!({"cadence":cadence,"periodStart":day,"currency":"CNY","utcOffsetMinutes":480})
}
pub(super) async fn verify(pool: &PgPool, app: &Router, f: &Fixture, cadence: &str) {
    let second = reviewer(pool, f).await;
    sqlx::query("UPDATE business_approval_policies SET min_approvers=2,allow_self_approval=true WHERE action_code='management_report:generate_snapshot'").execute(pool).await.unwrap();
    sqlx::raw_sql("CREATE FUNCTION pause_operating_vote() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.operation IN ('chat_document_approval_vote','agent_operating_snapshot_intent_prepared') AND NEW.target_type='operating_report_snapshot_intent' THEN PERFORM pg_advisory_xact_lock(202609204); END IF; RETURN NEW; END $$; CREATE TRIGGER pause_operating_vote BEFORE INSERT ON business_core_audit_events FOR EACH ROW EXECUTE FUNCTION pause_operating_vote();").execute(pool).await.unwrap();
    prepare_race(pool, app, f, cadence).await;
    let prepared = prepare(app, f.actor, KIND, input(cadence, "2026-07")).await;
    let before = counts(pool).await;
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(202609204)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let first = spawn(app, f.actor, &prepared);
    blocked(pool, 1).await;
    let next = spawn(app, second, &prepared);
    blocked(pool, 2).await;
    blocker.commit().await.unwrap();
    let (code, pending) = first.await.unwrap();
    assert_eq!(code, StatusCode::OK, "{pending}");
    assert_eq!(pending["executed"], false);
    let (code, done) = next.await.unwrap();
    assert_eq!(code, StatusCode::OK, "{done}");
    assert_eq!(done["executed"], true);
    assert_eq!(done["approvalCount"], 2);
    assert_eq!(
        counts(pool).await,
        (before.0 + 1, before.1 + 1, before.2 + 2)
    );
    // An already-cast vote cannot remain authoritative after its voter loses scope.
    let prepared = prepare(app, f.actor, KIND, input(cadence, "2026-06")).await;
    let (code, pending) = spawn(app, second, &prepared).await.unwrap();
    assert_eq!(code, StatusCode::OK, "{pending}");
    let before = counts(pool).await;
    sqlx::query("DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1")
        .bind(second)
        .execute(pool)
        .await
        .unwrap();
    let (code, result) = spawn(app, f.actor, &prepared).await.unwrap();
    assert_eq!(code, StatusCode::CONFLICT, "{result}");
    assert_eq!(counts(pool).await, before);
    sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$3)").bind(second).bind(f.customer).bind(f.actor).execute(pool).await.unwrap();
    let (code, result) = spawn(app, f.actor, &prepared).await.unwrap();
    assert_eq!(code, StatusCode::CONFLICT, "{result}");
    assert_eq!(counts(pool).await, before);
    // The intent expires after report creation while the final audit is blocked.
    sqlx::query("UPDATE business_approval_policies SET min_approvers=1 WHERE action_code='management_report:generate_snapshot'").execute(pool).await.unwrap();
    let mut expiring = prepare(app, f.actor, KIND, input(cadence, "2026-05")).await;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO business_agent_operating_snapshot_intents(id,kind,input,snapshot,created_by_user_id,idempotency_key,trace_id,expires_at) SELECT $1,kind,input,snapshot,created_by_user_id,$1::text,trace_id,clock_timestamp()+interval '2 seconds' FROM business_agent_operating_snapshot_intents WHERE id=$2")
        .bind(id).bind(expiring["item"]["id"].as_str().unwrap().parse::<Uuid>().unwrap()).execute(pool).await.unwrap();
    expiring["item"]["id"] = json!(id);
    let before = counts(pool).await;
    let writes_before = auxiliary_writes(pool).await;
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(202609204)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let expired = spawn(app, f.actor, &expiring);
    blocked(pool, 1).await;
    sqlx::query("SELECT pg_sleep(GREATEST(EXTRACT(EPOCH FROM (expires_at-clock_timestamp()))::double precision,0)+0.1) FROM business_agent_operating_snapshot_intents WHERE id=$1").bind(id).execute(pool).await.unwrap();
    blocker.commit().await.unwrap();
    let (code, result) = expired.await.unwrap();
    assert_eq!(code, StatusCode::NOT_FOUND, "{result}");
    assert_eq!(counts(pool).await, before);
    assert_eq!(auxiliary_writes(pool).await, writes_before);
    duplicate_confirmation(pool, app, f, cadence).await;
    revoke_during_wait(pool, app, f, cadence).await;
    sqlx::raw_sql("DROP TRIGGER pause_operating_vote ON business_core_audit_events; DROP FUNCTION pause_operating_vote();").execute(pool).await.unwrap();
}

async fn prepare_race(pool: &PgPool, app: &Router, f: &Fixture, cadence: &str) {
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(202609204)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let launch = || {
        let app = app.clone();
        let cadence = cadence.to_string();
        let input = input(&cadence, "2026-04");
        let actor = f.actor;
        tokio::spawn(async move {
            call(
                &app,
                actor,
                "POST",
                "/v1/agent-operating-snapshot-intents/operating_report_snapshot_intent",
                input,
                &format!("concurrent-operating-prepare-{cadence}"),
            )
            .await
        })
    };
    let first = launch();
    blocked(pool, 1).await;
    let second = launch();
    blocked(pool, 2).await;
    blocker.commit().await.unwrap();
    let (a, first) = first.await.unwrap();
    let (b, second) = second.await.unwrap();
    assert_eq!(a, StatusCode::OK, "{first}");
    assert_eq!(b, StatusCode::OK, "{second}");
    assert_eq!(first["item"]["id"], second["item"]["id"]);
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM business_core_audit_events WHERE operation='agent_operating_snapshot_intent_prepared' AND target_id=$1").bind(first["item"]["id"].as_str().unwrap()).fetch_one(pool).await.unwrap();
    assert_eq!(count, 1);
}
async fn revoke_during_wait(pool: &PgPool, app: &Router, f: &Fixture, cadence: &str) {
    let prepared = prepare(app, f.actor, KIND, input(cadence, "2026-03")).await;
    let before = counts(pool).await;
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT revision FROM business_authorization_revision WHERE singleton FOR UPDATE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let waiting = spawn(app, f.actor, &prepared);
    blocked(pool, 1).await;
    sqlx::query("DELETE FROM business_role_permissions WHERE permission_key='management_report:generate_snapshot'").execute(&mut *blocker).await.unwrap();
    blocker.commit().await.unwrap();
    let (code, result) = waiting.await.unwrap();
    assert_eq!(code, StatusCode::NOT_FOUND, "{result}");
    assert_eq!(counts(pool).await, before);
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,'management_report:generate_snapshot' FROM business_user_roles WHERE enterprise_user_id=$1").bind(f.actor).execute(pool).await.unwrap();
}

async fn auxiliary_writes(pool: &PgPool) -> (i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM business_core_audit_events),(SELECT count(*) FROM business_command_idempotency)").fetch_one(pool).await.unwrap()
}
async fn duplicate_confirmation(pool: &PgPool, app: &Router, f: &Fixture, cadence: &str) {
    let prepared = prepare(app, f.actor, KIND, json!({"cadence":cadence,"periodStart":"2026-02-02","currency":"CNY","utcOffsetMinutes":480})).await;
    let before = counts(pool).await;
    let writes = auxiliary_writes(pool).await;
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(202609204)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let first = spawn(app, f.actor, &prepared);
    blocked(pool, 1).await;
    let second = spawn(app, f.actor, &prepared);
    blocked(pool, 2).await;
    blocker.commit().await.unwrap();
    let (code, result) = first.await.unwrap();
    assert_eq!(code, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true);
    let (code, result) = second.await.unwrap();
    assert_eq!(code, StatusCode::CONFLICT, "{result}");
    assert_eq!(
        counts(pool).await,
        (before.0 + 1, before.1 + 1, before.2 + 1)
    );
    // One generation audit, one vote audit, and one idempotency record.
    assert_eq!(auxiliary_writes(pool).await, (writes.0 + 2, writes.1 + 1));
}
