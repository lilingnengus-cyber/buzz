use super::*;
const ORDINARY_SCOPES: [&str; 58] = [
    "core_master_creation_intent:create",
    "core_master_update_intent:create",
    "product_master_creation_intent:create",
    "product_master_update_intent:create",
    "crm:read",
    "crm_creation_intent:create",
    "crm_update_intent:create",
    "crm_followup_intent:create",
    "inventory_count_creation_intent:create",
    "inventory_count_submission_intent:create",
    "inventory_count_posting_intent:create",
    "inventory_count_cancellation_intent:create",
    "sales_return_reversal_intent:create",
    "sales_return_cancellation_intent:create",
    "purchase_return_reversal_intent:create",
    "purchase_return_cancellation_intent:create",
    "sales_return:update_draft",
    "purchase_return:update_draft",
    "sales_return:create",
    "purchase_return:create",
    "sales_return_inspection_intent:create",
    "purchase_return_dispatch_intent:create",
    "purchase_return_acknowledgment_intent:create",
    "sales_return:read",
    "purchase_return:read",
    "shipment_reversal_intent:create",
    "goods_receipt_reversal_intent:create",
    "inventory_opening_reversal_intent:create",
    "sales_order_cancellation_intent:create",
    "purchase_order_cancellation_intent:create",
    "customer_receipt_reversal_intent:create",
    "supplier_payment_reversal_intent:create",
    "receivable_allocation_reversal_intent:create",
    "payable_allocation_reversal_intent:create",
    "business_master_data:read",
    "sales_order:read",
    "purchase_order:read",
    "inventory:read",
    "customer_receipt:read",
    "supplier_payment:read",
    "shipment:read",
    "goods_receipt:read",
    "receivable:read",
    "payable:read",
    "order_profit:read",
    "business_anomaly:read",
    "business_action:read",
    "sales_order:update_draft",
    "purchase_order:update_draft",
    "inventory_opening:create",
    "receivable_allocation_intent:create",
    "payable_allocation_intent:create",
    "sales_order:create",
    "shipment:create",
    "purchase_order:create",
    "goods_receipt:create",
    "customer_receipt:create",
    "supplier_payment:create",
];

pub(super) async fn check(
    store: &Store,
    pool: &sqlx::PgPool,
    keys: &Keys,
    user: Uuid,
    binding: Uuid,
    human: Uuid,
) {
    let scopes = ORDINARY_SCOPES
        .iter()
        .map(|s| (*s).to_string())
        .collect::<Vec<_>>();
    let inserted:Vec<Uuid>=sqlx::query_scalar("INSERT INTO business_iam.principal_permissions(principal_id,permission_id) SELECT $1,id FROM business_iam.permissions WHERE capability=ANY($2) ON CONFLICT DO NOTHING RETURNING permission_id").bind(human).bind(&scopes).fetch_all(pool).await.unwrap();
    let channel = Uuid::new_v4().to_string();
    let event = EventBuilder::new(Kind::TextNote, "准备盘点能力验收")
        .tags([Tag::custom(TagKind::Custom("h".into()), [channel.clone()])])
        .sign_with_keys(keys)
        .unwrap();
    let issued = store
        .issue_agent_delegation(
            IssueAgentDelegationRequest {
                source_event: event.clone(),
                source_buzz_event_id: event.id.to_hex(),
                source_buzz_pubkey: event.pubkey.to_hex(),
                source_channel_id: channel,
                agent_id: "business-query-agent".into(),
                agent_turn_id: "count-capacity".into(),
                scopes: scopes.clone(),
            },
            facts(Uuid::new_v4()),
        )
        .await
        .unwrap();
    assert_eq!(
        issued.scopes.iter().collect::<HashSet<_>>(),
        scopes.iter().collect::<HashSet<_>>()
    );
    let stored: i32 =
        sqlx::query_scalar("SELECT cardinality(scopes) FROM agent_read_delegations WHERE id=$1")
            .bind(issued.id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(stored, 58);
    assert!(sqlx::query("UPDATE agent_read_delegations SET scopes=array_fill('inventory:read'::text,ARRAY[129]) WHERE id=$1").bind(issued.id).execute(pool).await.is_err());
    super::inventory_count_budget::check(pool, keys, user, binding).await;
    sqlx::query("DELETE FROM business_iam.principal_permissions WHERE principal_id=$1 AND permission_id=ANY($2)").bind(human).bind(inserted).execute(pool).await.unwrap();
    for (family, name) in [
        ("core_master", "creation"),
        ("core_master", "update"),
        ("product_master", "creation"),
        ("product_master", "update"),
        ("inventory_count", "creation"),
        ("inventory_count", "submission"),
        ("inventory_count", "posting"),
        ("inventory_count", "cancellation"),
        ("crm", "creation"),
        ("crm", "update"),
        ("crm", "followup"),
    ] {
        let scope = format!("{family}_{name}_intent:approve");
        let kind = format!("{family}_{name}_intent");
        let permission:Uuid=sqlx::query_scalar("INSERT INTO business_iam.principal_permissions(principal_id,permission_id) SELECT $1,id FROM business_iam.permissions WHERE capability=$2 RETURNING permission_id").bind(human).bind(&scope).fetch_one(pool).await.unwrap();
        for (word, decision) in [("确认", "approve"), ("拒绝", "reject")] {
            let id = Uuid::new_v4();
            let trace = Uuid::new_v4();
            let channel = Uuid::new_v4().to_string();
            let turn = format!("{family}-{name}-{decision}");
            let hash = "c".repeat(64);
            let event = EventBuilder::new(
                Kind::TextNote,
                format!("{word} {} {id} v1 {hash}", kind.replace('_', "-")),
            )
            .tags([Tag::custom(TagKind::Custom("h".into()), [channel.clone()])])
            .sign_with_keys(keys)
            .unwrap();
            let issued = store
                .issue_agent_delegation(
                    IssueAgentDelegationRequest {
                        source_event: event.clone(),
                        source_buzz_event_id: event.id.to_hex(),
                        source_buzz_pubkey: event.pubkey.to_hex(),
                        source_channel_id: channel,
                        agent_id: "business-query-agent".into(),
                        agent_turn_id: turn.clone(),
                        scopes: vec![scope.clone()],
                    },
                    facts(trace),
                )
                .await
                .unwrap();
            let consumed = store
                .consume_agent_delegation(
                    &issued.token,
                    ConsumeAgentDelegationRequest {
                        tool_name: format!("approve_{family}_{name}"),
                        required_scope: scope.clone(),
                        agent_id: "business-query-agent".into(),
                        agent_turn_id: turn.clone(),
                    },
                    facts(trace),
                )
                .await
                .unwrap();
            assert_eq!(
                consumed.approval_document_type.as_deref(),
                Some(kind.as_str())
            );
            assert_eq!(consumed.approval_document_id, Some(id));
            assert_eq!(consumed.approval_decision.as_deref(), Some(decision));
            for mode in 0..5 {
                let candidate = business_auth_gateway::agent::VerifyApproval {
                    document_id: if mode == 1 { Uuid::new_v4() } else { id },
                    expected_version: if mode == 2 { 2 } else { 1 },
                    preview_hash: if mode == 3 {
                        "0".repeat(64)
                    } else {
                        hash.clone()
                    },
                    decision: if mode == 4 {
                        if decision == "approve" {
                            "reject"
                        } else {
                            "approve"
                        }
                        .into()
                    } else {
                        decision.into()
                    },
                };
                let result = store
                    .verify_agent_delegation(
                        VerifyAgentDelegationRequest {
                            delegation_id: issued.id,
                            enterprise_user_id: user,
                            identity_binding_id: binding,
                            agent_id: "business-query-agent".into(),
                            agent_turn_id: turn.clone(),
                            trace_id: trace,
                            used_calls: 1,
                            required_scope: scope.clone(),
                            approval: Some(candidate),
                        },
                        facts(trace),
                    )
                    .await;
                assert_eq!(result.is_ok(), mode == 0, "{name} {decision} mode {mode}");
            }
        }
        sqlx::query("DELETE FROM business_iam.principal_permissions WHERE principal_id=$1 AND permission_id=$2").bind(human).bind(permission).execute(pool).await.unwrap();
    }
}
