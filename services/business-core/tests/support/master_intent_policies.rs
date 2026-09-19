use super::*;
async fn totals(pool: &PgPool) -> (i64, i64, i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM business_brands),(SELECT count(*) FROM business_brand_scopes),(SELECT count(*) FROM business_document_approval_requests),(SELECT count(*) FROM business_document_approval_votes),(SELECT count(*) FROM business_core_audit_events)").fetch_one(pool).await.unwrap()
}
async fn brand(app: &Router, actor: Uuid) -> Value {
    prepare(app,actor,"product_master_creation_intent",json!({"operation":"create","command":{"resourceType":"brand","code":Uuid::new_v4().simple().to_string().to_uppercase(),"name":"Policy brand"}})).await
}
async fn submit(app: &Router, actor: Uuid, prepared: &Value) -> (StatusCode, Value) {
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
pub async fn check(
    pool: &PgPool,
    app: &Router,
    actor: Uuid,
    role: Uuid,
    entries: &[(bool, Uuid, Value)],
) {
    let other = Uuid::new_v4();
    let third = Uuid::new_v4();
    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'master-intents',$1::text,'Other approver'),($2,'master-intents',$2::text,'Third approver')").bind(other).bind(third).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) VALUES($1,$2,$3),($4,$2,$3)").bind(other).bind(role).bind(actor).bind(third).execute(pool).await.unwrap();
    let prepared = brand(app, actor).await;
    for change in [
        "UPDATE business_approval_policies SET step_up_amount_minor=0 WHERE action_code='business_product_master:manage'",
        "UPDATE business_approval_policies SET step_up_amount_minor=NULL,allow_self_approval=false WHERE action_code='business_product_master:manage'",
        "UPDATE business_approval_policies SET allow_self_approval=true,eligible_role_keys=ARRAY['not_eligible'] WHERE action_code='business_product_master:manage'",
        "UPDATE business_approval_policies SET eligible_role_keys=ARRAY['master_intent_approver'],require_distinct_business_unit=true WHERE action_code='business_product_master:manage'",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(change)).execute(pool).await.unwrap();
        let before=totals(pool).await;
        assert_eq!(submit(app,actor,&prepared).await.0,StatusCode::NOT_FOUND);
        assert_eq!(totals(pool).await,before);
    }
    // Global objects do not fabricate a business unit to satisfy distinct-unit policy.
    assert_eq!(submit(app, other, &prepared).await.0, StatusCode::NOT_FOUND);
    sqlx::query("UPDATE business_approval_policies SET require_distinct_business_unit=false,allow_self_approval=false WHERE action_code='business_product_master:manage'").execute(pool).await.unwrap();
    let (status, result) = submit(app, other, &prepared).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true);

    // Original thresholds are retained; strengthened current thresholds take effect.
    sqlx::query("UPDATE business_approval_policies SET allow_self_approval=true,min_approvers=2 WHERE action_code='business_product_master:manage'").execute(pool).await.unwrap();
    let prepared = brand(app, actor).await;
    let first_vote = vote(&prepared);
    let path = approval_path("product_master_creation_intent", &prepared);
    let (status, result) = call(app, actor, "POST", &path, first_vote.clone(), "").await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["status"], "pending");
    assert_eq!(result["executed"], false);
    assert_eq!(
        call(app, actor, "POST", &path, first_vote.clone(), "")
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(app, other, "POST", &path, first_vote, "").await.0,
        StatusCode::CONFLICT
    );
    sqlx::query("DELETE FROM business_user_roles WHERE enterprise_user_id=$1 AND role_id=$2")
        .bind(actor)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
    let before = totals(pool).await;
    assert_eq!(submit(app, other, &prepared).await.0, StatusCode::NOT_FOUND);
    assert_eq!(totals(pool).await, before);
    sqlx::query(
        "INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) VALUES($1,$2,$1)",
    )
    .bind(actor)
    .bind(role)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE business_approval_policies SET min_approvers=3 WHERE action_code='business_product_master:manage'").execute(pool).await.unwrap();
    let (status, result) = submit(app, other, &prepared).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["status"], "pending");
    assert_eq!(result["minimumApprovers"], 3);
    sqlx::query("UPDATE business_approval_policies SET min_approvers=1 WHERE action_code='business_product_master:manage'").execute(pool).await.unwrap();
    let (status, result) = submit(app, third, &prepared).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true);
    assert_eq!(result["minimumApprovers"], 3);

    let prepared = brand(app, actor).await;
    let mut rejected = vote(&prepared);
    rejected["decision"] = json!("reject");
    let (status, result) = call(
        app,
        other,
        "POST",
        &approval_path("product_master_creation_intent", &prepared),
        rejected,
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["status"], "rejected");
    assert_eq!(result["executed"], false);
    assert_eq!(submit(app, actor, &prepared).await.0, StatusCode::CONFLICT);

    // Concurrent confirmations of the same creation can produce only one record.
    let prepared = brand(app, actor).await;
    let before = totals(pool).await;
    let (one, two) = tokio::join!(submit(app, actor, &prepared), submit(app, other, &prepared));
    assert_eq!(
        usize::from(one.0 == StatusCode::OK) + usize::from(two.0 == StatusCode::OK),
        1
    );
    let after = totals(pool).await;
    assert_eq!(after.0, before.0 + 1);
    assert_eq!(after.3, before.3 + 1);

    // Business save, new-object scope grant, vote and approval status all roll back
    // if the final approval-state mutation fails after the actual insert.
    let prepared = brand(app, actor).await;
    let command = vote(&prepared);
    let path = approval_path("product_master_creation_intent", &prepared);
    sqlx::query("CREATE FUNCTION master_test_abort_execution() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.status='executed' THEN RAISE EXCEPTION 'isolated atomicity check'; END IF; RETURN NEW; END $$").execute(pool).await.unwrap();
    sqlx::query("CREATE TRIGGER master_test_abort_execution BEFORE UPDATE ON business_document_approval_requests FOR EACH ROW EXECUTE FUNCTION master_test_abort_execution()").execute(pool).await.unwrap();
    let before = totals(pool).await;
    assert_eq!(
        call(app, actor, "POST", &path, command.clone(), "").await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(totals(pool).await, before);
    sqlx::query("DROP TRIGGER master_test_abort_execution ON business_document_approval_requests")
        .execute(pool)
        .await
        .unwrap();
    let (status, result) = call(app, actor, "POST", &path, command, "").await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true);

    let (_, customer, fields) = entries
        .iter()
        .find(|(_, _, f)| f["resourceType"] == "customer")
        .unwrap();
    let mut fields = fields.clone();
    fields["expectedVersion"] = json!(2);
    fields["name"] = json!("Approval scope test");
    let prepared = prepare(
        app,
        actor,
        "core_master_update_intent",
        json!({"operation":"update","documentId":customer,"command":fields}),
    )
    .await;
    let path = approval_path("core_master_update_intent", &prepared);
    assert_eq!(
        call(app, other, "POST", &path, vote(&prepared), "").await.0,
        StatusCode::NOT_FOUND
    );
    let mut wrong = vote(&prepared);
    wrong["previewHash"] = json!("a".repeat(64));
    assert_eq!(
        call(app, actor, "POST", &path, wrong, "").await.0,
        StatusCode::CONFLICT
    );
    let mut wrong = vote(&prepared);
    wrong["expectedVersion"] = json!(2);
    assert_eq!(
        call(app, actor, "POST", &path, wrong, "").await.0,
        StatusCode::CONFLICT
    );
    sqlx::query("UPDATE business_customers SET name='Concurrent customer edit' WHERE id=$1")
        .bind(customer)
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(
        call(app, actor, "POST", &path, vote(&prepared), "").await.0,
        StatusCode::CONFLICT
    );

    // Immutable expiry and strict command family parsing.
    let prepared = brand(app, actor).await;
    let id = Uuid::parse_str(prepared["item"]["id"].as_str().unwrap()).unwrap();
    let expired = Uuid::new_v4();
    sqlx::query("INSERT INTO business_agent_master_intents(id,kind,input,snapshot,created_by_user_id,idempotency_key,trace_id,expires_at) SELECT $1,kind,input,snapshot,created_by_user_id,$1::text,trace_id,clock_timestamp()-interval '1 second' FROM business_agent_master_intents WHERE id=$2").bind(expired).bind(id).execute(pool).await.unwrap();
    let (status, _) = call(
        app,
        actor,
        "POST",
        &format!("/v1/agent-approvals/master/product_master_creation_intent/{expired}"),
        vote(&prepared),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let invalid = json!({"operation":"create","command":{"resourceType":"brand","code":"INVALID","name":"Invalid","execute":true}});
    assert_eq!(
        call(
            app,
            actor,
            "POST",
            "/v1/agent-master-intents/product_master_creation_intent",
            invalid,
            &Uuid::new_v4().to_string()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(call(app,actor,"POST","/v1/agent-master-intents/product_master_status_intent",json!({"operation":"change_status","resourceType":"brand","documentId":id,"command":{"status":"disabled","expectedVersion":1}}),&Uuid::new_v4().to_string()).await.0,StatusCode::NOT_FOUND);
}
