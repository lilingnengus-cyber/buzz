use super::{call, vote};
use axum::{http::StatusCode, Router};
use business_core::{
    b2::DomainError,
    crm::{CrmCommand, CrmService},
    PgStore,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

async fn state(pool: &PgPool, id: Uuid) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('item',to_jsonb(o),'notes',(SELECT jsonb_agg(to_jsonb(f) ORDER BY f.id) FROM crm_followups f WHERE opportunity_id=o.id),'audit',(SELECT count(*) FROM business_core_audit_events WHERE target_type='crm_opportunity' AND target_id=o.id::text)) FROM crm_opportunities o WHERE id=$1")
        .bind(id).fetch_one(pool).await.unwrap()
}
pub(super) async fn check(app: &Router, pool: &PgPool, actor: Uuid, customer: Uuid, id: Uuid) {
    let input = json!({"operation":"followup","opportunityId":id,"command":{
        "note":"Approved follow-up","stage":"won","nextAction":"Prepare contract","expectedVersion":3}});
    let (status, prepared) = call(
        app,
        actor,
        "POST",
        "/v1/agent-crm-intents/crm_followup_intent",
        input.clone(),
        "crm-failure-intent-0001",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{prepared}");
    let path = format!(
        "/v1/agent-approvals/crm/crm_followup_intent/{}",
        prepared["item"]["id"].as_str().unwrap()
    );
    // Parent display/terms are bound even when the opportunity version is unchanged.
    let name: String = sqlx::query_scalar("SELECT name FROM business_customers WHERE id=$1")
        .bind(customer)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE business_customers SET name='Changed after preview' WHERE id=$1")
        .bind(customer)
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(
        call(
            app,
            actor,
            "POST",
            &path,
            vote(&prepared),
            "crm-failure-intent-0001"
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    sqlx::query("UPDATE business_customers SET name=$2 WHERE id=$1")
        .bind(customer)
        .bind(name)
        .execute(pool)
        .await
        .unwrap();
    // Restoring a name still advances its updatedAt; obtain a new bound preview.
    let (status, prepared) = call(
        app,
        actor,
        "POST",
        "/v1/agent-crm-intents/crm_followup_intent",
        input.clone(),
        "crm-failure-intent-0002",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{prepared}");
    let path = format!(
        "/v1/agent-approvals/crm/crm_followup_intent/{}",
        prepared["item"]["id"].as_str().unwrap()
    );
    let before = state(pool, id).await;
    // Fail after the note insert: opportunity update, note and executed outcome
    // must not survive the failed transaction.
    sqlx::raw_sql("CREATE FUNCTION crm_test_fail() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'isolated CRM failure'; END $$; CREATE TRIGGER crm_test_fail AFTER INSERT ON crm_followups FOR EACH ROW EXECUTE FUNCTION crm_test_fail();").execute(pool).await.unwrap();
    let result = call(
        app,
        actor,
        "POST",
        &path,
        vote(&prepared),
        "crm-failure-intent-0001",
    )
    .await;
    sqlx::raw_sql("DROP TRIGGER crm_test_fail ON crm_followups; DROP FUNCTION crm_test_fail();")
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(result.0, StatusCode::CONFLICT, "{:?}", result.1);
    assert_eq!(state(pool, id).await, before);
    let status: String = sqlx::query_scalar(
        "SELECT status FROM business_document_approval_requests WHERE document_id=$1",
    )
    .bind(Uuid::parse_str(prepared["item"]["id"].as_str().unwrap()).unwrap())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(status, "execution_failed");

    let crm = CrmService::new(PgStore::new(pool.clone()));
    let command: CrmCommand = serde_json::from_value(input).unwrap();
    let snapshot = crm.command_preview(actor, &command).await.unwrap();
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM crm_opportunities WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    let running = tokio::spawn(async move {
        crm.execute_guarded(
            actor,
            Uuid::new_v4(),
            "crm-locked-preview-0001",
            &command,
            &snapshot,
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10),async {
        loop {
            let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) AND query LIKE 'SELECT * FROM crm_opportunities%')")
                .bind(pid).fetch_one(pool).await.unwrap();
            if waiting {break;}
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    // Preserve version to prove full-snapshot binding, rather than only the
    // existing optimistic version check, catches a changed source.
    sqlx::query("UPDATE crm_opportunities SET contact_details='changed while waiting' WHERE id=$1")
        .bind(id)
        .execute(&mut *blocker)
        .await
        .unwrap();
    blocker.commit().await.unwrap();
    assert!(matches!(
        running.await.unwrap(),
        Err(DomainError::StalePreview)
    ));
    let after = state(pool, id).await;
    assert_eq!(after["notes"], before["notes"]);
    assert_eq!(after["audit"], before["audit"]);
    assert_eq!(after["item"]["version"], before["item"]["version"]);
    assert_eq!(after["item"]["contact_details"], "changed while waiting");
}
