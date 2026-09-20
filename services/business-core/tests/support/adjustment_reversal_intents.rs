use super::*;
#[path = "adjustment_reversal_intent_expiry.rs"]
mod expiry;
const REVERSE: &str = "operational_adjustment_reversal_intent";
async fn posted(pool: &PgPool, f: &Fixture, order: Uuid, key: &str) -> Uuid {
    let id = fixture::draft(pool, f, order, key).await;
    let service =
        business_core::b4::AdjustmentService::new(PgStore::new(pool.clone()), "ADJ".into(), 500);
    let version = business_core::b4::model::VersionCommand {
        expected_version: 1,
    };
    let preview = service
        .allocation_preview(f.actor, id, &version)
        .await
        .unwrap();
    service
        .post_guarded(f.actor, Uuid::new_v4(), id, key, &version, &preview)
        .await
        .unwrap();
    id
}
pub async fn verify(
    pool: &PgPool,
    app: &Router,
    f: &Fixture,
    order: Uuid,
    first: Uuid,
    second: Uuid,
) {
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,'profit_adjustment:reverse' FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).execute(pool).await.unwrap();
    let id = posted(pool, f, order, "reverse-intent-source").await;
    let input = json!({"batchId":id,"expectedVersion":3,"reason":"纠正重复费用"});
    let before = counts(pool).await;
    let (code, dry) = call(
        app,
        f.actor,
        "POST",
        &format!("/v1/agent-adjustment-previews/{REVERSE}"),
        input.clone(),
        "",
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{dry}");
    assert_eq!(counts(pool).await, before);
    assert_eq!(dry["document"]["effects"]["preservesOriginalFacts"], true);
    let mut invalid = input.clone();
    invalid["reason"] = json!("");
    assert_eq!(
        call(
            app,
            f.actor,
            "POST",
            &format!("/v1/agent-adjustment-intents/{REVERSE}"),
            invalid,
            "reverse-empty-reason"
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let p = prepare(app, f.actor, REVERSE, input).await;
    let path = approval_path(REVERSE, &p);
    assert_eq!(p["document"], dry["document"]);
    assert_eq!(
        call(app, first, "POST", &path, vote(&p), "").await.0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES('profit_adjustment:reverse','profit_adjustment:reverse',ARRAY['b2_operator'],2,false)").execute(pool).await.unwrap();
    assert_eq!(
        call(app, f.actor, "POST", &path, vote(&p), "").await.0,
        StatusCode::NOT_FOUND
    );
    let mut wrong = vote(&p);
    wrong["previewHash"] = json!("0".repeat(64));
    assert_eq!(
        call(app, first, "POST", &path, wrong, "").await.0,
        StatusCode::CONFLICT
    );
    let mut wrong = vote(&p);
    wrong["reason"] = json!("替换原因");
    assert_eq!(
        call(app, first, "POST", &path, wrong, "").await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let (code, pending) = call(app, first, "POST", &path, vote(&p), "").await;
    assert_eq!(code, StatusCode::OK, "{pending}");
    assert_eq!(pending["status"], "pending");
    assert!(pending["reversedDocument"].is_null());
    sqlx::query(
        "DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2",
    )
    .bind(first)
    .bind(f.customer)
    .execute(pool)
    .await
    .unwrap();
    let before = counts(pool).await;
    assert_eq!(
        call(app, second, "POST", &path, vote(&p), "").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(counts(pool).await, before);
    sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$3)").bind(first).bind(f.customer).bind(f.actor).execute(pool).await.unwrap();
    sqlx::raw_sql("CREATE FUNCTION fail_reverse_vote() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.operation='chat_document_approval_vote' AND NEW.target_type='operational_adjustment_reversal_intent' THEN RAISE EXCEPTION 'injected reversal vote failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER fail_reverse_vote BEFORE INSERT ON business_core_audit_events FOR EACH ROW EXECUTE FUNCTION fail_reverse_vote();").execute(pool).await.unwrap();
    assert_eq!(
        call(app, second, "POST", &path, vote(&p), "").await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(counts(pool).await, before);
    sqlx::raw_sql("DROP TRIGGER fail_reverse_vote ON business_core_audit_events; DROP FUNCTION fail_reverse_vote();").execute(pool).await.unwrap();
    let (code, done) = call(app, second, "POST", &path, vote(&p), "").await;
    assert_eq!(code, StatusCode::OK, "{done}");
    assert_eq!(done["reversedDocument"]["id"], json!(id));
    assert_eq!(done["reversedDocument"]["status"], "reversed");
    assert_eq!(done["reversedDocument"]["version"], 4);
    assert!(done.get("postedDocument").is_none());
    let exact:(i64,String)=sqlx::query_as("SELECT count(*),sum(CASE direction WHEN 'normal' THEN amount ELSE -amount END)::text FROM profit_facts WHERE source_type='operational_adjustment' AND source_id=$1").bind(id).fetch_one(pool).await.unwrap();
    assert_eq!(exact, (2, "0.000000".into()));
    let reason:String=sqlx::query_scalar("SELECT details->>'reason' FROM business_core_audit_events WHERE target_id=$1 AND operation='OPERATIONAL_ADJUSTMENT_REVERSED'").bind(id.to_string()).fetch_one(pool).await.unwrap();
    assert_eq!(reason, "纠正重复费用");
    assert_eq!(
        call(app, second, "POST", &path, vote(&p), "").await.0,
        StatusCode::CONFLICT
    );
    for mode in ["reject", "concurrent", "stale"] {
        let id = posted(pool, f, order, &format!("reverse-intent-{mode}")).await;
        let p = prepare(
            app,
            f.actor,
            REVERSE,
            json!({"batchId":id,"expectedVersion":3,"reason":"审批场景"}),
        )
        .await;
        let path = approval_path(REVERSE, &p);
        if mode == "concurrent" {
            let (a, b) = tokio::join!(
                call(app, first, "POST", &path, vote(&p), ""),
                call(app, second, "POST", &path, vote(&p), "")
            );
            assert_eq!(a.0, StatusCode::OK, "{}", a.1);
            assert_eq!(b.0, StatusCode::OK, "{}", b.1);
            assert_ne!(a.1["executed"], b.1["executed"]);
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM profit_facts WHERE source_id=$1 AND direction='reversal'",
            )
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap();
            assert_eq!(count, 1);
        } else if mode == "reject" {
            let mut command = vote(&p);
            command["decision"] = json!("reject");
            let (code, r) = call(app, first, "POST", &path, command, "").await;
            assert_eq!(code, StatusCode::OK, "{r}");
            assert_eq!(r["status"], "rejected");
            assert!(r["reversedDocument"].is_null());
            let status: String =
                sqlx::query_scalar("SELECT status FROM operational_adjustment_batches WHERE id=$1")
                    .bind(id)
                    .fetch_one(pool)
                    .await
                    .unwrap();
            assert_eq!(status, "posted");
        } else {
            sqlx::query("UPDATE operational_adjustment_batches SET version=version+1 WHERE id=$1")
                .bind(id)
                .execute(pool)
                .await
                .unwrap();
            let before = counts(pool).await;
            assert_eq!(
                call(app, first, "POST", &path, vote(&p), "").await.0,
                StatusCode::CONFLICT
            );
            assert_eq!(counts(pool).await, before);
        }
    }
    sqlx::query("UPDATE business_approval_policies SET min_approvers=1 WHERE action_code='profit_adjustment:reverse'").execute(pool).await.unwrap();
    expiry::verify(pool, app, f, order, first).await;
}
