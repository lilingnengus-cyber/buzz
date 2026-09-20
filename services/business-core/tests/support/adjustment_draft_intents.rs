use super::*;
#[path = "adjustment_draft_intent_expiry.rs"]
mod expiry;
const CREATE: &str = "operational_adjustment_creation_intent";
const UPDATE: &str = "operational_adjustment_update_intent";
async fn footprint(pool: &PgPool) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('batches',(SELECT jsonb_agg(to_jsonb(b) ORDER BY id) FROM operational_adjustment_batches b),'lines',(SELECT jsonb_agg(to_jsonb(l) ORDER BY id) FROM operational_adjustment_lines l),'requests',(SELECT count(*) FROM business_document_approval_requests),'votes',(SELECT count(*) FROM business_document_approval_votes),'audit',(SELECT count(*) FROM business_core_audit_events),'idem',(SELECT count(*) FROM business_command_idempotency),'facts',(SELECT count(*) FROM profit_facts),'outbox',(SELECT count(*) FROM business_core_outbox),'events',(SELECT count(*) FROM operational_adjustment_events),'previews',(SELECT count(*) FROM operational_adjustment_previews),'allocations',(SELECT count(*) FROM operational_adjustment_allocations),'numbering',(SELECT COALESCE(sum(current_value),0) FROM business_numbering_sequence_pools))").fetch_one(pool).await.unwrap()
}
pub async fn verify(
    pool: &PgPool,
    app: &Router,
    f: &Fixture,
    order: Uuid,
    first: Uuid,
    second: Uuid,
) {
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,'profit_adjustment:update_draft' FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).execute(pool).await.unwrap();
    let input = json!({"legalEntityId":f.legal_entity,"currency":"CNY","managementPeriod":"2026-08","lines":[{"metricType":"allocated_operating_expense","amount":"10.01","businessDate":"2026-08-21","allocationBasis":"direct","directSalesOrderId":order,"reasonCode":"TEST"}]});
    let before = footprint(pool).await;
    let (code, dry) = call(
        app,
        f.actor,
        "POST",
        &format!("/v1/agent-adjustment-previews/{CREATE}"),
        input.clone(),
        "",
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{dry}");
    assert_eq!(footprint(pool).await, before);
    let prepared = prepare(app, f.actor, CREATE, input.clone()).await;
    let batches: i64 = sqlx::query_scalar("SELECT count(*) FROM operational_adjustment_batches")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(batches, 0, "preparation does not create drafts");
    let path = approval_path(CREATE, &prepared);
    assert_eq!(
        call(app, first, "POST", &path, vote(&prepared), "").await.0,
        StatusCode::NOT_FOUND
    );
    for action in ["profit_adjustment:create", "profit_adjustment:update_draft"] {
        sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES($1,$1,ARRAY['b2_operator'],2,false)").bind(action).execute(pool).await.unwrap();
    }
    assert_eq!(
        call(app, f.actor, "POST", &path, vote(&prepared), "")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (code, pending) = call(app, first, "POST", &path, vote(&prepared), "").await;
    assert_eq!(code, StatusCode::OK, "{pending}");
    assert_eq!(pending["executed"], false);
    assert!(pending["createdDocument"].is_null());
    let before = footprint(pool).await;
    // Final audit failure must roll back the draft, numbering, vote and request transition.
    sqlx::raw_sql("CREATE FUNCTION fail_draft_vote() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.operation='chat_document_approval_vote' AND NEW.target_type IN ('operational_adjustment_creation_intent','operational_adjustment_update_intent') THEN RAISE EXCEPTION 'injected draft vote failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER fail_draft_vote BEFORE INSERT ON business_core_audit_events FOR EACH ROW EXECUTE FUNCTION fail_draft_vote();").execute(pool).await.unwrap();
    assert_eq!(
        call(app, second, "POST", &path, vote(&prepared), "")
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(footprint(pool).await, before);
    sqlx::raw_sql("DROP TRIGGER fail_draft_vote ON business_core_audit_events; DROP FUNCTION fail_draft_vote();").execute(pool).await.unwrap();
    let (code, done) = call(app, second, "POST", &path, vote(&prepared), "").await;
    assert_eq!(code, StatusCode::OK, "{done}");
    assert_eq!(done["createdDocument"]["status"], "draft");
    assert_eq!(done["createdDocument"]["version"], 1);
    assert!(done.get("postedDocument").is_none());
    assert_eq!(
        call(app, second, "POST", &path, vote(&prepared), "")
            .await
            .0,
        StatusCode::CONFLICT
    );
    let batch = done["createdDocument"]["id"].clone();
    let mut changed = input.clone();
    changed["lines"][0]["amount"] = json!("20.02");
    let update = json!({"batchId":batch,"expectedVersion":1,"batch":changed});
    let prepared = prepare(app, f.actor, UPDATE, update.clone()).await;
    let path = approval_path(UPDATE, &prepared);
    let mut bad = vote(&prepared);
    bad["amount"] = json!("999");
    assert_eq!(
        call(app, first, "POST", &path, bad, "").await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let (a, b) = tokio::join!(
        call(app, first, "POST", &path, vote(&prepared), ""),
        call(app, second, "POST", &path, vote(&prepared), "")
    );
    assert_eq!(a.0, StatusCode::OK, "{}", a.1);
    assert_eq!(b.0, StatusCode::OK, "{}", b.1);
    assert_ne!(a.1["executed"], b.1["executed"]);
    let done = if a.1["executed"] == true { a.1 } else { b.1 };
    assert_eq!(done["updatedDocument"]["id"], batch);
    assert_eq!(done["updatedDocument"]["version"], 2);
    let record:(String,String)=sqlx::query_as("SELECT b.status,l.amount::text FROM operational_adjustment_batches b JOIN operational_adjustment_lines l ON l.batch_id=b.id WHERE b.id=$1").bind(batch.as_str().unwrap().parse::<Uuid>().unwrap()).fetch_one(pool).await.unwrap();
    assert_eq!(record, ("draft".into(), "20.020000".into()));
    // Rejecting creation does not create another batch.
    let rejected = prepare(app, f.actor, CREATE, input.clone()).await;
    let before = footprint(pool).await;
    let mut no = vote(&rejected);
    no["decision"] = json!("reject");
    let (code, result) = call(
        app,
        first,
        "POST",
        &approval_path(CREATE, &rejected),
        no,
        "",
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{result}");
    assert_eq!(result["status"], "rejected");
    assert!(result["createdDocument"].is_null());
    let after = footprint(pool).await;
    assert_eq!(before["batches"], after["batches"]);
    assert_eq!(before["numbering"], after["numbering"]);
    assert_eq!(before["facts"], after["facts"]);
    // Immutable intent and expired intent both fail closed.
    let id = rejected["item"]["id"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    assert!(sqlx::query(
        "UPDATE business_agent_adjustment_intents SET snapshot=snapshot WHERE id=$1"
    )
    .bind(id)
    .execute(pool)
    .await
    .is_err());
    let expired = Uuid::new_v4();
    sqlx::query("INSERT INTO business_agent_adjustment_intents(id,kind,input,snapshot,created_by_user_id,idempotency_key,trace_id,expires_at) SELECT $1,kind,input,snapshot,created_by_user_id,$1::text,trace_id,clock_timestamp()-interval '1 second' FROM business_agent_adjustment_intents WHERE id=$2").bind(expired).bind(id).execute(pool).await.unwrap();
    let mut expired_value = rejected.clone();
    expired_value["item"]["id"] = json!(expired);
    let before = footprint(pool).await;
    assert_eq!(
        call(
            app,
            first,
            "POST",
            &approval_path(CREATE, &expired_value),
            vote(&expired_value),
            ""
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(footprint(pool).await, before);
    sqlx::query("UPDATE business_approval_policies SET min_approvers=1 WHERE action_code IN ('profit_adjustment:create','profit_adjustment:update_draft')").execute(pool).await.unwrap();
    expiry::verify(pool, app, f, first, CREATE, input.clone()).await;
    expiry::verify(
        pool,
        app,
        f,
        first,
        UPDATE,
        json!({"batchId":batch,"expectedVersion":2,"batch":input}),
    )
    .await;
    // A source modified after prepare invalidates replacement before recording a vote.
    let prepared = prepare(
        app,
        f.actor,
        UPDATE,
        json!({"batchId":batch,"expectedVersion":2,"batch":input}),
    )
    .await;
    sqlx::query(
        "UPDATE operational_adjustment_batches SET management_period='2026-09' WHERE id=$1",
    )
    .bind(batch.as_str().unwrap().parse::<Uuid>().unwrap())
    .execute(pool)
    .await
    .unwrap();
    let before = footprint(pool).await;
    assert_eq!(
        call(
            app,
            first,
            "POST",
            &approval_path(UPDATE, &prepared),
            vote(&prepared),
            ""
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(footprint(pool).await, before);
}
