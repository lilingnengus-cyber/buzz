use super::*;

pub(super) async fn check(
    app: &Router,
    store: &PgStore,
    f: &Fixture,
    sales: bool,
    original: &business_core::b2::CreateReturn,
) {
    concurrent_edit_and_create(store, f, sales, original).await;
    let kind = if sales {
        "sales_return"
    } else {
        "purchase_return"
    };
    let mut draft = serde_json::to_value(original).unwrap();
    draft["expectedSourceVersion"] = json!(2);
    draft["lines"][0]["quantity"] = json!("0.5");
    let path = format!("/v1/agent-drafts/returns/{kind}");
    let (status, first) = call(app, f.actor, "POST", &path, draft.clone()).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let id = first["id"].as_str().unwrap();
    let edit_path = format!("{path}/{id}");
    let edit_source = format!("/v1/agent-return-edit-sources/{kind}/{id}");
    let (status, source) = call(app, f.actor, "GET", &edit_source, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{source}");
    assert_eq!(source["item"]["id"], original.source_id.to_string());

    let mut replacement = draft.clone();
    replacement.as_object_mut().unwrap().remove("sourceId");
    replacement["expectedVersion"] = json!(1);
    replacement["lines"][0]["quantity"] = json!("0.75");
    replacement["businessNote"] = json!("修改后的草稿");
    for (field, value, expected) in [
        ("expectedVersion", json!(8), StatusCode::CONFLICT),
        ("expectedSourceVersion", json!(8), StatusCode::CONFLICT),
        (
            "sourceId",
            json!(Uuid::new_v4()),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let mut invalid = replacement.clone();
        invalid[field] = value;
        assert_eq!(
            call(app, f.actor, "PUT", &edit_path, invalid).await.0,
            expected
        );
    }
    for quantity in ["0", "-1", "1.1"] {
        let mut invalid = replacement.clone();
        invalid["lines"][0]["quantity"] = json!(quantity);
        assert!(!call(app, f.actor, "PUT", &edit_path, invalid)
            .await
            .0
            .is_success());
    }
    let mut invalid = replacement.clone();
    invalid["lines"][0]["sourceLineId"] = json!(Uuid::new_v4());
    assert_eq!(
        call(app, f.actor, "PUT", &edit_path, invalid).await.0,
        StatusCode::NOT_FOUND
    );
    let mut invalid = replacement.clone();
    invalid["lines"]
        .as_array_mut()
        .unwrap()
        .push(replacement["lines"][0].clone());
    assert_eq!(
        call(app, f.actor, "PUT", &edit_path, invalid).await.0,
        StatusCode::BAD_REQUEST
    );
    sqlx::query("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2")
        .bind(f.actor)
        .bind(f.brand)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        call(app, f.actor, "PUT", &edit_path, replacement.clone())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(app, f.actor, "GET", &edit_source, Value::Null).await.0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.brand).execute(store.pool()).await.unwrap();
    let key = format!("return-edit-{}", Uuid::new_v4());
    let (status, updated) =
        call_key(app, f.actor, "PUT", &edit_path, replacement.clone(), &key).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["version"], 2);
    let (status, replay) =
        call_key(app, f.actor, "PUT", &edit_path, replacement.clone(), &key).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["idempotentReplay"], true);
    assert_eq!(replay["id"], first["id"]);
    let lookup = format!("/v1/agent-return-documents/{kind}?documentId={id}");
    let (_, found) = call(app, f.actor, "GET", &lookup, Value::Null).await;
    assert_eq!(found["items"][0]["businessNote"], "修改后的草稿");
    assert_eq!(found["items"][0]["version"], 2);
    assert_eq!(
        found["items"][0]["lines"][0]["quantity"]
            .as_str()
            .unwrap()
            .parse::<Decimal>()
            .unwrap(),
        Decimal::new(75, 2)
    );
    draft["lines"][0]["quantity"] = json!("0.25");
    let (status, other) = call(app, f.actor, "POST", &path, draft).await;
    assert_eq!(status, StatusCode::OK, "{other}");
    let other_id = other["id"].as_str().unwrap();
    assert_eq!(
        call_key(
            app,
            f.actor,
            "PUT",
            &format!("{path}/{other_id}"),
            replacement.clone(),
            &key
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    replacement["expectedVersion"] = json!(2);
    replacement["lines"][0]["quantity"] = json!("1");
    assert_eq!(
        call(app, f.actor, "PUT", &edit_path, replacement.clone())
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    cancel(store, f, sales, other_id.parse().unwrap(), 1).await;
    let (status, updated) = call(app, f.actor, "PUT", &edit_path, replacement.clone()).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["version"], 3);
    cancel(store, f, sales, id.parse().unwrap(), 3).await;
    replacement["expectedVersion"] = json!(4);
    assert_eq!(
        call(app, f.actor, "PUT", &edit_path, replacement).await.0,
        StatusCode::BAD_REQUEST
    );
    let source_path = format!("/v1/agent-return-sources/{kind}/{}", original.source_id);
    let (_, source) = call(app, f.actor, "GET", &source_path, Value::Null).await;
    assert_eq!(
        source["item"]["lines"][0]["returnableQuantity"]
            .as_str()
            .unwrap()
            .parse::<Decimal>()
            .unwrap(),
        Decimal::ONE
    );
}

async fn cancel(store: &PgStore, f: &Fixture, sales: bool, id: Uuid, version: i64) {
    let service =
        business_core::b2::ReturnService::new(store.clone(), "SRET".into(), "PRET".into());
    let command = business_core::b2::model::VersionCommand {
        expected_version: version,
        reason_code: Some("验收后取消草稿".into()),
    };
    let key = format!("cancel-edit-fixture-{}", Uuid::new_v4());
    let result = if sales {
        service
            .cancel_sales_return(f.actor, Uuid::new_v4(), id, &key, &command)
            .await
    } else {
        service
            .cancel_purchase_return(f.actor, Uuid::new_v4(), id, &key, &command)
            .await
    };
    assert_eq!(result.unwrap().status, "cancelled");
}

async fn concurrent_edit_and_create(
    store: &PgStore,
    f: &Fixture,
    sales: bool,
    original: &business_core::b2::CreateReturn,
) {
    use business_core::b2::{ReplaceReturnDraft, ReturnService};
    let service = ReturnService::new(store.clone(), "SRET".into(), "PRET".into());
    let mut initial = original.clone();
    initial.expected_source_version = Some(2);
    initial.lines[0].quantity.0 = Decimal::new(5, 1);
    let first = if sales {
        service
            .create_sales_return(f.actor, Uuid::new_v4(), "edit-race-initial-sales", &initial)
            .await
    } else {
        service
            .create_purchase_return(
                f.actor,
                Uuid::new_v4(),
                "edit-race-initial-purchase",
                &initial,
            )
            .await
    }
    .unwrap();
    let mut blocker = store.pool().begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query(if sales {
        "SELECT id FROM shipments WHERE id=$1 FOR UPDATE"
    } else {
        "SELECT id FROM goods_receipts WHERE id=$1 FOR UPDATE"
    })
    .bind(original.source_id)
    .fetch_one(&mut *blocker)
    .await
    .unwrap();
    let actor = f.actor;
    let mut edit = serde_json::to_value(&initial).unwrap();
    edit.as_object_mut().unwrap().remove("sourceId");
    edit["expectedVersion"] = json!(1);
    edit["lines"][0]["quantity"] = json!("1");
    let edit: ReplaceReturnDraft = serde_json::from_value(edit).unwrap();
    let edit_service = service.clone();
    let editing = tokio::spawn(async move {
        edit_service
            .replace_draft(
                actor,
                Uuid::new_v4(),
                sales,
                first.id,
                "edit-race-replacement",
                &edit,
            )
            .await
    });
    let creating = tokio::spawn(async move {
        if sales {
            service
                .create_sales_return(actor, Uuid::new_v4(), "edit-race-create-sales", &initial)
                .await
        } else {
            service
                .create_purchase_return(
                    actor,
                    Uuid::new_v4(),
                    "edit-race-create-purchase",
                    &initial,
                )
                .await
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(10),async {
        loop {
            let waiting:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity a WHERE a.datname=current_database() AND ($1=ANY(pg_blocking_pids(a.pid)) OR EXISTS(SELECT 1 FROM pg_stat_activity b WHERE b.datname=current_database() AND b.pid=ANY(pg_blocking_pids(a.pid)) AND $1=ANY(pg_blocking_pids(b.pid))))").bind(pid).fetch_one(store.pool()).await.unwrap();
            if waiting>=2 { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("edit and creation both wait for source allocation lock");
    blocker.commit().await.unwrap();
    let edited = editing.await.unwrap();
    let created = creating.await.unwrap();
    assert_ne!(
        edited.is_ok(),
        created.is_ok(),
        "only one may consume the remaining half-unit: {edited:?} / {created:?}"
    );
    let first_version = match edited {
        Ok(result) => result.version,
        Err(DomainError::Invalid(message)) if message.contains("remainder") => 1,
        other => panic!("unexpected edit result: {other:?}"),
    };
    match created {
        Ok(result) => cancel(store, f, sales, result.id, result.version).await,
        Err(DomainError::Invalid(message)) if message.contains("remainder") => (),
        other => panic!("unexpected create result: {other:?}"),
    }
    cancel(store, f, sales, first.id, first_version).await;
}
