#[path = "agent_allocations.rs"]
mod allocations;
use super::*;

pub(super) async fn check(app: &Router, store: &PgStore, f: &Fixture, supplier: Uuid) {
    for capability in [
        "supplier_payment:read",
        "supplier_payment:create",
        "supplier_payment:confirm",
        "payable_allocation:create",
    ] {
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,$2 FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).bind(capability).execute(store.pool()).await.unwrap();
    }
    for (kind, path, party_key, party, date_key, event) in [
        (
            "customer_receipt",
            "customer-receipts",
            "customerId",
            f.customer,
            "receiptDate",
            '3',
        ),
        (
            "supplier_payment",
            "supplier-payments",
            "supplierId",
            supplier,
            "paymentDate",
            '4',
        ),
    ] {
        let mut draft = json!({"legalEntityId":f.legal_entity,"currency":"CNY","amount":"123.45","paymentMethod":"bank_transfer","externalReference":"settlement-fixture"});
        draft[party_key] = json!(party);
        draft[date_key] = json!("2026-09-19");
        let (status, created) = call(
            app,
            f.actor,
            "POST",
            &format!("/v1/agent-drafts/{path}"),
            draft,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{created}");
        let id = created["id"].as_str().unwrap();
        let preview_path = format!("/v1/agent-approval-previews/settlement/{kind}/{id}");
        let (status, preview) = call(app, f.actor, "GET", &preview_path, Value::Null).await;
        assert_eq!(status, StatusCode::OK, "{preview}");
        assert_eq!(preview["item"]["amount"], "123.450000");
        assert_eq!(preview["item"]["executesBankTransfer"], false);
        assert_eq!(preview["document"][party_key], party.to_string());
        let approval_path = format!("/v1/agent-approvals/settlement/{kind}/{id}");
        let command = json!({"expectedVersion":1,"previewHash":preview["previewHash"],"decision":"approve","sourceBuzzEventId":event.to_string().repeat(64),"sourceChannelId":"settlement-test"});
        let (status, _) = call(app, f.actor, "POST", &approval_path, command.clone()).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "missing policy denies");
        sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES($1,$1,ARRAY['b2_operator'],1,true)").bind(format!("{kind}:confirm")).execute(store.pool()).await.unwrap();
        // A saved preview must not survive revocation of a party's current scope.
        let revoke = if kind == "customer_receipt" {
            "DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2"
        } else {
            "DELETE FROM business_supplier_scopes WHERE enterprise_user_id=$1 AND supplier_id=$2"
        };
        sqlx::query(revoke)
            .bind(f.actor)
            .bind(party)
            .execute(store.pool())
            .await
            .unwrap();
        let (status, hidden) = call(
            app,
            f.actor,
            "GET",
            &format!("/v1/agent-financial-documents/{kind}?documentId={id}"),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{hidden}");
        assert_eq!(
            hidden["items"],
            json!([]),
            "revoked party is absent from lookup"
        );
        let (status, _) = call(app, f.actor, "GET", &preview_path, Value::Null).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(app, f.actor, "POST", &approval_path, command.clone()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let restore = if kind == "customer_receipt" {
            "INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)"
        } else {
            "INSERT INTO business_supplier_scopes(enterprise_user_id,supplier_id,granted_by) VALUES($1,$2,$1)"
        };
        sqlx::query(restore)
            .bind(f.actor)
            .bind(party)
            .execute(store.pool())
            .await
            .unwrap();
        let mut stale = command.clone();
        stale["previewHash"] = json!("0".repeat(64));
        let (status, _) = call(app, f.actor, "POST", &approval_path, stale).await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, result) = call(app, f.actor, "POST", &approval_path, command.clone()).await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(result["executed"], true);
        assert_eq!(
            result["resourceRefs"][0]["bizUri"],
            format!("biz://{}/{id}", kind.replace('_', "-"))
        );
        let (status, _) = call(app, f.actor, "POST", &approval_path, command).await;
        assert_eq!(status, StatusCode::CONFLICT, "cannot confirm twice");
        let (_, after) = call(app, f.actor, "GET", &preview_path, Value::Null).await;
        assert_eq!(after["item"]["status"], "confirmed");
        assert_eq!(after["item"]["unappliedAmount"], "123.450000");
        assert_eq!(after["item"]["allocatedAmount"], "0.000000");
        allocations::check(app, store, f, kind, party, id).await;
    }
}
