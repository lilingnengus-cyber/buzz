#[path = "agent_reversals.rs"]
mod reversals;
use super::*;
use sqlx::{AssertSqlSafe, Row};
// Dynamic identifiers below are selected solely from the two literal fixture families.

pub(super) async fn check(
    app: &Router,
    store: &PgStore,
    f: &Fixture,
    source_kind: &str,
    party: Uuid,
    source_id: &str,
) {
    let (kind, table, party_column, action, event) = if source_kind == "customer_receipt" {
        (
            "receivable_allocation_intent",
            "trade_receivables",
            "customer_id",
            "receivable_allocation:create",
            '5',
        )
    } else {
        (
            "payable_allocation_intent",
            "trade_payables",
            "supplier_id",
            "payable_allocation:create",
            '6',
        )
    };
    let row=sqlx::query(AssertSqlSafe(format!("SELECT id,version,open_amount FROM {table} WHERE {party_column}=$1 AND open_amount>=50 AND status IN ('open','partially_settled') ORDER BY recognized_at DESC LIMIT 1"))).bind(party).fetch_one(store.pool()).await.unwrap();
    let target: Uuid = row.get("id");
    let version: i64 = row.get("version");
    let open: Decimal = row.get("open_amount");
    let target_kind = if source_kind == "customer_receipt" {
        "receivable"
    } else {
        "payable"
    };
    for (family, id, expected) in [
        (source_kind, source_id.to_owned(), 2),
        (target_kind, target.to_string(), version),
    ] {
        let route = format!(
            "/v1/agent-financial-documents/{family}?documentId={id}&partyId={party}&limit=1"
        );
        let (status, found) = call(app, f.actor, "GET", &route, Value::Null).await;
        assert_eq!(status, StatusCode::OK, "{found}");
        assert_eq!(found["items"].as_array().unwrap().len(), 1);
        assert_eq!(found["items"][0]["id"], id);
        assert_eq!(found["items"][0]["version"], expected);
        let amount_key = if family == source_kind {
            "unappliedAmount"
        } else {
            "openAmount"
        };
        assert!(found["items"][0][amount_key].is_string());
        for suffix in ["&offset=1", "&query=nonexistent", "&status=nonexistent"] {
            let (status, none) = call(
                app,
                f.actor,
                "GET",
                &format!("{route}{suffix}"),
                Value::Null,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{none}");
            assert_eq!(none["items"], json!([]));
        }
        assert_eq!(
            call(
                app,
                f.actor,
                "GET",
                &format!("{route}&arbitrary=sql"),
                Value::Null
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    let path = format!("/v1/agent-allocation-intents/{kind}");
    let mut body = json!({"sourceDocumentId":source_id,"expectedSourceVersion":2,"allocations":[{"documentId":target,"expectedVersion":version,"amount":"50"}]});
    let key = format!("prepare-allocation-{}", Uuid::new_v4());
    let (status, preview) = call_key(app, f.actor, "POST", &path, body.clone(), &key).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["document"]["totalAmount"], "50");
    let intent = preview["item"]["id"].as_str().unwrap().to_owned();
    let (_, replayed) = call_key(app, f.actor, "POST", &path, body.clone(), &key).await;
    assert_eq!(replayed["item"]["id"], intent, "idempotent preparation");
    let mut changed = body.clone();
    changed["allocations"][0]["amount"] = json!("49");
    assert_eq!(
        call_key(app, f.actor, "POST", &path, changed, &key).await.0,
        StatusCode::CONFLICT
    );
    assert!(
        sqlx::query("UPDATE business_agent_allocation_intents SET input='{}' WHERE id=$1")
            .bind(intent.parse::<Uuid>().unwrap())
            .execute(store.pool())
            .await
            .is_err(),
        "prepared payload must be immutable"
    );
    sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES($1,$1,ARRAY['b2_operator'],1,true) ON CONFLICT DO NOTHING").bind(action).execute(store.pool()).await.unwrap();
    let command = json!({"expectedVersion":1,"previewHash":preview["previewHash"],"decision":"approve","sourceBuzzEventId":event.to_string().repeat(64),"sourceChannelId":"allocation-test"});
    let approve_path = format!("/v1/agent-approvals/allocations/{kind}/{intent}");
    let mut tampered = command.clone();
    tampered["allocations"] = json!([]);
    assert_eq!(
        call(app, f.actor, "POST", &approve_path, tampered).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    // Hold the target after the preview so execution waits after its initial revalidation.
    let mut lock = store.pool().begin().await.unwrap();
    sqlx::query(AssertSqlSafe(format!(
        "SELECT id FROM {table} WHERE id=$1 FOR UPDATE"
    )))
    .bind(target)
    .fetch_one(&mut *lock)
    .await
    .unwrap();
    let app_copy = app.clone();
    let actor = f.actor;
    let run =
        tokio::spawn(async move { call(&app_copy, actor, "POST", &approve_path, command).await });
    let mut executing = false;
    for _ in 0..100 {
        executing=sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM business_document_approval_requests WHERE document_id=$1 AND status='executing')").bind(intent.parse::<Uuid>().unwrap()).fetch_one(store.pool()).await.unwrap();
        if executing {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(
        executing,
        "confirmation must reach the transaction before releasing lock"
    );
    sqlx::query(AssertSqlSafe(format!(
        "UPDATE {table} SET trace_id=$2 WHERE id=$1"
    )))
    .bind(target)
    .bind(Uuid::new_v4())
    .execute(&mut *lock)
    .await
    .unwrap();
    lock.commit().await.unwrap();
    assert_eq!(
        run.await.unwrap().0,
        StatusCode::CONFLICT,
        "target version changed while confirmation waited for lock"
    );
    let source_table = if source_kind == "customer_receipt" {
        "customer_receipts"
    } else {
        "supplier_payments"
    };
    let allocated: Decimal = sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT allocated_amount FROM {source_table} WHERE id=$1"
    )))
    .bind(source_id.parse::<Uuid>().unwrap())
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(allocated, Decimal::ZERO);
    body["allocations"][0]["expectedVersion"] = json!(version + 1);
    let (_, fresh) = call(app, f.actor, "POST", &path, body).await;
    let fresh_id = fresh["item"]["id"].as_str().unwrap();
    let command = json!({"expectedVersion":1,"previewHash":fresh["previewHash"],"decision":"approve","sourceBuzzEventId":if source_kind=="customer_receipt"{"7".repeat(64)}else{"8".repeat(64)},"sourceChannelId":"allocation-test"});
    let approve_path = format!("/v1/agent-approvals/allocations/{kind}/{fresh_id}");
    let (status, result) = call(app, f.actor, "POST", &approve_path, command.clone()).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["executed"], true);
    assert_eq!(
        call(app, f.actor, "POST", &approve_path, command).await.0,
        StatusCode::CONFLICT
    );
    let balance: Decimal = sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT open_amount FROM {table} WHERE id=$1"
    )))
    .bind(target)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(balance, open - Decimal::from(50));
    reversals::check(app, store, f, source_kind, source_id, target, open).await;
}
