use super::*;

pub(super) async fn check(
    app: &Router,
    store: &PgStore,
    f: &Fixture,
    source_kind: &str,
    source_id: &str,
    target: Uuid,
    original_open: Decimal,
) {
    let (target_kind, allocation_kind) = if source_kind == "customer_receipt" {
        ("receivable", "receivable_allocation")
    } else {
        ("payable", "payable_allocation")
    };
    for action in [
        format!("{source_kind}:reverse"),
        format!("{allocation_kind}:reverse"),
    ] {
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,$2 FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).bind(action).execute(store.pool()).await.unwrap();
    }
    let (_, source) = call(
        app,
        f.actor,
        "GET",
        &format!("/v1/agent-financial-documents/{source_kind}?documentId={source_id}"),
        Value::Null,
    )
    .await;
    let (_, target_view) = call(
        app,
        f.actor,
        "GET",
        &format!("/v1/agent-financial-documents/{target_kind}?documentId={target}"),
        Value::Null,
    )
    .await;
    let source_version = source["items"][0]["version"].as_i64().unwrap();
    let mut target_version = target_view["items"][0]["version"].as_i64().unwrap();
    let history_path =
        format!("/v1/agent-allocation-history/{source_kind}?sourceDocumentId={source_id}&limit=1");
    let (status, history) = call(app, f.actor, "GET", &history_path, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    assert_eq!(history["items"].as_array().unwrap().len(), 1);
    assert_eq!(history["items"][0]["reversed"], false);
    assert_eq!(history["items"][0]["source"]["version"], source_version);
    assert_eq!(history["items"][0]["target"]["version"], target_version);
    assert!(history["items"][0]["amount"].is_string());
    let allocation: Uuid = history["items"][0]["id"].as_str().unwrap().parse().unwrap();
    let (_, empty) = call(
        app,
        f.actor,
        "GET",
        &format!("{history_path}&offset=1"),
        Value::Null,
    )
    .await;
    assert_eq!(empty["items"], json!([]));
    let reason = format!("录入错误纠正 {}", Uuid::new_v4());
    let source_path = format!("/v1/agent-reversal-intents/{source_kind}_reversal_intent");
    let source_body = json!({"sourceDocumentId":source_id,"expectedSourceVersion":source_version,"reason":reason});
    assert!(
        !call(app, f.actor, "POST", &source_path, source_body)
            .await
            .0
            .is_success(),
        "allocated source cannot be reversed"
    );
    let stale_kind = format!("{allocation_kind}_reversal_intent");
    let (_,stale)=call(app,f.actor,"POST",&format!("/v1/agent-reversal-intents/{stale_kind}"),json!({"sourceDocumentId":source_id,"expectedSourceVersion":source_version,"allocationId":allocation,"expectedTargetVersion":target_version,"reason":reason})).await;
    let stale_id = stale["item"]["id"].as_str().unwrap();
    assert!(
        sqlx::query("UPDATE business_agent_reversal_intents SET input='{}' WHERE id=$1")
            .bind(stale_id.parse::<Uuid>().unwrap())
            .execute(store.pool())
            .await
            .is_err()
    );
    let table = if source_kind == "customer_receipt" {
        "trade_receivables"
    } else {
        "trade_payables"
    };
    sqlx::query(AssertSqlSafe(format!(
        "UPDATE {table} SET trace_id=$2 WHERE id=$1"
    )))
    .bind(target)
    .bind(Uuid::new_v4())
    .execute(store.pool())
    .await
    .unwrap();
    let (status,_)=call(app,f.actor,"POST",&format!("/v1/agent-approvals/reversals/{stale_kind}/{stale_id}"),json!({"expectedVersion":1,"previewHash":stale["previewHash"],"decision":"approve","sourceBuzzEventId":Uuid::new_v4().simple().to_string().repeat(2),"sourceChannelId":"reversal-test"})).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "changed target invalidates a saved reversal"
    );
    target_version += 1;
    for (base, body) in [
        (
            allocation_kind,
            json!({"sourceDocumentId":source_id,"expectedSourceVersion":source_version,"allocationId":allocation,"expectedTargetVersion":target_version,"reason":reason}),
        ),
        (
            source_kind,
            json!({"sourceDocumentId":source_id,"expectedSourceVersion":source_version+1,"reason":reason}),
        ),
    ] {
        let kind = format!("{base}_reversal_intent");
        let path = format!("/v1/agent-reversal-intents/{kind}");
        let mut invalid = body.clone();
        invalid["reason"] = json!("  ");
        assert_eq!(
            call(app, f.actor, "POST", &path, invalid).await.0,
            StatusCode::BAD_REQUEST
        );
        let key = Uuid::new_v4().to_string();
        let (status, preview) = call_key(app, f.actor, "POST", &path, body.clone(), &key).await;
        assert_eq!(status, StatusCode::OK, "{preview}");
        assert_eq!(preview["document"]["reason"], reason);
        assert_eq!(preview["document"]["executesBankTransfer"], false);
        let (_, replayed) = call_key(app, f.actor, "POST", &path, body.clone(), &key).await;
        assert_eq!(preview["item"]["id"], replayed["item"]["id"]);
        let mut changed = body;
        changed["reason"] = json!("不同原因");
        assert_eq!(
            call_key(app, f.actor, "POST", &path, changed, &key).await.0,
            StatusCode::CONFLICT
        );
        let id = preview["item"]["id"].as_str().unwrap();
        let approval = format!("/v1/agent-approvals/reversals/{kind}/{id}");
        let event = Uuid::new_v4().simple().to_string().repeat(2);
        let command = json!({"expectedVersion":1,"previewHash":preview["previewHash"],"decision":"approve","sourceBuzzEventId":event,"sourceChannelId":"reversal-test"});
        assert_eq!(
            call(app, f.actor, "POST", &approval, command.clone())
                .await
                .0,
            StatusCode::NOT_FOUND,
            "missing policy denies"
        );
        let action = format!("{base}:reverse");
        sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES($1,$1,ARRAY['b2_operator'],1,true) ON CONFLICT DO NOTHING").bind(action).execute(store.pool()).await.unwrap();
        let mut tampered = command.clone();
        tampered["reason"] = json!("changed after preview");
        assert_eq!(
            call(app, f.actor, "POST", &approval, tampered).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let mut stale = command.clone();
        stale["previewHash"] = json!("0".repeat(64));
        assert_eq!(
            call(app, f.actor, "POST", &approval, stale).await.0,
            StatusCode::CONFLICT
        );
        let (status, result) = call(app, f.actor, "POST", &approval, command.clone()).await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(result["executed"], true);
        assert_eq!(
            call(app, f.actor, "POST", &approval, command).await.0,
            StatusCode::CONFLICT
        );
        let (_, found) = call(
            app,
            f.actor,
            "GET",
            &format!("/v1/agent-financial-documents/{target_kind}?documentId={target}"),
            Value::Null,
        )
        .await;
        assert_eq!(
            found["items"][0]["openAmount"]
                .as_str()
                .unwrap()
                .parse::<Decimal>()
                .unwrap(),
            original_open
        );
    }
    let (_, history) = call(app, f.actor, "GET", &history_path, Value::Null).await;
    assert_eq!(history["items"][0]["reversed"], true);
    let (_, source) = call(
        app,
        f.actor,
        "GET",
        &format!("/v1/agent-financial-documents/{source_kind}?documentId={source_id}"),
        Value::Null,
    )
    .await;
    assert_eq!(source["items"][0]["status"], "reversed");
    assert_eq!(
        source["items"][0]["unappliedAmount"]
            .as_str()
            .unwrap()
            .parse::<Decimal>()
            .unwrap(),
        Decimal::ZERO
    );
    let audits:i64=sqlx::query_scalar("SELECT count(*) FROM business_core_audit_events WHERE actor_user_id=$1 AND details->>'reason'=$2").bind(f.actor).bind(reason).fetch_one(store.pool()).await.unwrap();
    assert_eq!(
        audits, 2,
        "both underlying reversal transactions retain their reason"
    );
}
