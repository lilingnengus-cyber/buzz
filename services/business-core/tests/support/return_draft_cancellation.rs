use super::*;

pub(super) async fn check(
    app: &Router,
    store: &PgStore,
    f: &Fixture,
    sales: bool,
    original: &business_core::b2::CreateReturn,
) {
    let family = if sales {
        "sales_return"
    } else {
        "purchase_return"
    };
    let kind = format!("{family}_cancellation_intent");
    let path = format!("/v1/agent-return-disposition-intents/{kind}");
    let mut draft = serde_json::to_value(original).unwrap();
    draft["expectedSourceVersion"] = json!(2);
    let (status, created) = call(
        app,
        f.actor,
        "POST",
        &format!("/v1/agent-drafts/returns/{family}"),
        draft.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap().parse::<Uuid>().unwrap();
    let command = json!({"expectedVersion":1,"reason":"取消重复退货草稿"});
    let (status, prepared) = call(
        app,
        f.actor,
        "POST",
        &path,
        json!({"sourceDocumentId":id,"command":command}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{prepared}");
    assert_eq!(
        prepared["document"]["source"]["lines"][0]["returnableQuantityReleased"]
            .as_str()
            .unwrap()
            .parse::<Decimal>()
            .unwrap(),
        Decimal::ONE
    );
    for key in [
        "inventoryQuantityChange",
        "inventoryValueChange",
        "receivableChange",
        "payableChange",
    ] {
        assert_eq!(prepared["document"]["effects"][key], "0");
    }
    let inventory_before: i64 = sqlx::query_scalar("SELECT count(*) FROM inventory_movements")
        .fetch_one(store.pool())
        .await
        .unwrap();
    let financial_sql = if sales {
        "SELECT jsonb_build_array(original_amount,settled_amount,open_amount,version) FROM trade_receivables WHERE shipment_id=$1"
    } else {
        "SELECT jsonb_build_array(original_amount,settled_amount,open_amount,version) FROM trade_payables WHERE goods_receipt_id=$1"
    };
    let financial_before: Value = sqlx::query_scalar(financial_sql)
        .bind(original.source_id)
        .fetch_one(store.pool())
        .await
        .unwrap();

    // Hold approval insertion after preview validation, then make a real draft edit.
    sqlx::query("CREATE OR REPLACE FUNCTION test_return_cancel_gate() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.document_type IN ('sales_return_cancellation_intent','purchase_return_cancellation_intent') THEN PERFORM pg_advisory_xact_lock(76543219); END IF; RETURN NEW; END $$").execute(store.pool()).await.unwrap();
    sqlx::query("CREATE TRIGGER test_return_cancel_gate BEFORE INSERT ON business_document_approval_requests FOR EACH ROW EXECUTE FUNCTION test_return_cancel_gate()").execute(store.pool()).await.unwrap();
    let mut blocker = store.pool().begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(76543219)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let approval_path = format!(
        "/v1/agent-approvals/return-dispositions/{kind}/{}",
        prepared["item"]["id"].as_str().unwrap()
    );
    let approval = json!({"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":"approve","sourceBuzzEventId":Uuid::new_v4().simple().to_string().repeat(2),"sourceChannelId":"return-cancel-fixture"});
    let worker = app.clone();
    let actor = f.actor;
    let task =
        tokio::spawn(async move { call(&worker, actor, "POST", &approval_path, approval).await });
    tokio::time::timeout(std::time::Duration::from_secs(10),async {
        loop {let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)))").bind(pid).fetch_one(store.pool()).await.unwrap();if waiting {break;}tokio::time::sleep(std::time::Duration::from_millis(10)).await;}
    }).await.expect("approval reached verified lock wait after preview");
    draft.as_object_mut().unwrap().remove("sourceId");
    draft["expectedVersion"] = json!(1);
    draft["businessNote"] = json!("确认前修改");
    let (status, changed) = call(
        app,
        f.actor,
        "PUT",
        &format!("/v1/agent-drafts/returns/{family}/{id}"),
        draft,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed["version"], 2);
    blocker.commit().await.unwrap();
    let (status, rejected) = task.await.unwrap();
    assert_eq!(status, StatusCode::CONFLICT, "{rejected}");
    sqlx::query("DROP TRIGGER test_return_cancel_gate ON business_document_approval_requests")
        .execute(store.pool())
        .await
        .unwrap();
    let (_, current) = call(
        app,
        f.actor,
        "GET",
        &format!("/v1/agent-return-documents/{family}?documentId={id}"),
        Value::Null,
    )
    .await;
    assert_eq!(current["items"][0]["status"], "draft");
    let execution_key = return_disposition_checks::execute(
        app,
        store,
        f,
        id,
        &kind,
        json!({"expectedVersion":2,"reason":"取消重复退货草稿"}),
    )
    .await;
    let (_, current) = call(
        app,
        f.actor,
        "GET",
        &format!("/v1/agent-return-documents/{family}?documentId={id}"),
        Value::Null,
    )
    .await;
    assert_eq!(current["items"][0]["status"], "cancelled");
    assert_eq!(current["items"][0]["version"], 3);
    assert_eq!(
        call(
            app,
            f.actor,
            "POST",
            &path,
            json!({"sourceDocumentId":id,"command":{"expectedVersion":3,"reason":"repeat"}})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    let service =
        business_core::b2::ReturnService::new(store.clone(), "SRET".into(), "PRET".into());
    let replay_command = business_core::b2::model::VersionCommand {
        expected_version: 2,
        reason_code: Some("取消重复退货草稿".into()),
    };
    let replay = if sales {
        service
            .cancel_sales_return(f.actor, Uuid::new_v4(), id, &execution_key, &replay_command)
            .await
    } else {
        service
            .cancel_purchase_return(f.actor, Uuid::new_v4(), id, &execution_key, &replay_command)
            .await
    }
    .unwrap();
    assert!(replay.idempotent_replay);
    let mut other = original.clone();
    other.expected_source_version = Some(2);
    let other = if sales {
        service
            .create_sales_return(f.actor, Uuid::new_v4(), "cancel-cross-id-sales", &other)
            .await
    } else {
        service
            .create_purchase_return(f.actor, Uuid::new_v4(), "cancel-cross-id-purchase", &other)
            .await
    }
    .unwrap();
    let cross = if sales {
        service
            .cancel_sales_return(
                f.actor,
                Uuid::new_v4(),
                other.id,
                &execution_key,
                &replay_command,
            )
            .await
    } else {
        service
            .cancel_purchase_return(
                f.actor,
                Uuid::new_v4(),
                other.id,
                &execution_key,
                &replay_command,
            )
            .await
    };
    assert!(
        matches!(
            cross,
            Err(business_core::b2::DomainError::IdempotencyConflict)
        ),
        "{cross:?}"
    );
    let cleanup = business_core::b2::model::VersionCommand {
        expected_version: 1,
        reason_code: Some("cleanup".into()),
    };
    if sales {
        service
            .cancel_sales_return(
                f.actor,
                Uuid::new_v4(),
                other.id,
                "cleanup-cross-sales",
                &cleanup,
            )
            .await
    } else {
        service
            .cancel_purchase_return(
                f.actor,
                Uuid::new_v4(),
                other.id,
                "cleanup-cross-purchase",
                &cleanup,
            )
            .await
    }
    .unwrap();
    let inventory_after: i64 = sqlx::query_scalar("SELECT count(*) FROM inventory_movements")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(inventory_before, inventory_after);
    let financial_after: Value = sqlx::query_scalar(financial_sql)
        .bind(original.source_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(financial_before, financial_after);
    let reason:String=sqlx::query_scalar(if sales {"SELECT payload->>'reason' FROM sales_return_events WHERE sales_return_id=$1 AND event_type='cancelled'"}else{"SELECT payload->>'reason' FROM purchase_return_events WHERE purchase_return_id=$1 AND event_type='cancelled'"}).bind(id).fetch_one(store.pool()).await.unwrap();
    assert_eq!(reason, "取消重复退货草稿");
}
