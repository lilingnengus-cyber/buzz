use super::*;
pub(super) async fn verify(
    pool: &sqlx::PgPool,
    core: &CoreClient,
    f: &crate::test_fixture::Fixture,
    order: Uuid,
) {
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,'profit_adjustment:update_draft' FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).execute(pool).await.unwrap();
    for action in ["profit_adjustment:create", "profit_adjustment:update_draft"] {
        sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES($1,$1,ARRAY['b2_operator'],1,true)").bind(action).execute(pool).await.unwrap();
    }
    for create in [true, false] {
        for (decision, minimum) in [("approve", 1_i16), ("reject", 1_i16), ("approve", 2_i16)] {
            let (kind, tool, approve, action) = if create {
                (
                    "operational_adjustment_creation_intent",
                    "prepare_operational_adjustment_creation",
                    "approve_operational_adjustment_creation",
                    "profit_adjustment:create",
                )
            } else {
                (
                    "operational_adjustment_update_intent",
                    "prepare_operational_adjustment_update",
                    "approve_operational_adjustment_update",
                    "profit_adjustment:update_draft",
                )
            };
            sqlx::query(
                "UPDATE business_approval_policies SET min_approvers=$1 WHERE action_code=$2",
            )
            .bind(minimum)
            .bind(action)
            .execute(pool)
            .await
            .unwrap();
            let batch = json!({"legalEntityId":f.legal_entity,"currency":"CNY","managementPeriod":"2026-08","lines":[{"metricType":"allocated_operating_expense","amount":"10.01","businessDate":"2026-08-21","allocationBasis":"direct","directSalesOrderId":order,"customerId":f.customer,"businessUnitId":f.business_unit,"brandId":f.brand,"warehouseId":f.warehouse,"reasonCode":"TEST"}]});
            let source = if create {
                None
            } else {
                let service = business_core::b4::AdjustmentService::new(
                    PgStore::new(pool.clone()),
                    "ADJ".into(),
                    500,
                );
                Some(
                    service
                        .create(
                            f.actor,
                            Uuid::new_v4(),
                            &Uuid::new_v4().to_string(),
                            &serde_json::from_value(batch.clone()).unwrap(),
                        )
                        .await
                        .unwrap()
                        .id,
                )
            };
            let input = if create {
                batch.clone()
            } else {
                let mut b = batch.clone();
                b["lines"][0]["amount"] = json!("20.02");
                json!({"batchId":source,"expectedVersion":1,"batch":b})
            };
            assert!(valid(tool, &input));
            assert!(WRITE_TOOLS.contains(&tool));
            assert!(is_approval_tool(approve));
            let c = context(f.actor, required_capability(tool).unwrap());
            let allowed = grant(&c, f.legal_entity);
            let before: i64 =
                sqlx::query_scalar("SELECT count(*) FROM business_agent_adjustment_intents")
                    .fetch_one(pool)
                    .await
                    .unwrap();
            for dimension in [
                "legal_entity",
                "customer",
                "business_unit",
                "warehouse",
                "brand",
                "supplier",
            ] {
                let mut denied = allowed.clone();
                if let DataScope::Restricted(dims) = &mut denied.data_scope {
                    dims.insert(dimension.into(), [Uuid::new_v4().to_string()].into());
                }
                assert_eq!(
                    forward(core, tool, input.clone(), &c, &denied)
                        .await
                        .status(),
                    StatusCode::FORBIDDEN,
                    "{tool} {dimension}"
                );
            }
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT count(*) FROM business_agent_adjustment_intents"
                )
                .fetch_one(pool)
                .await
                .unwrap(),
                before
            );
            if create {
                let mut unassigned = input.clone();
                unassigned["lines"][0]["brandId"] = Value::Null;
                let mut restricted = allowed.clone();
                if let DataScope::Restricted(dims) = &mut restricted.data_scope {
                    dims.insert("brand".into(), [f.brand.to_string()].into());
                }
                assert_eq!(
                    forward(core, tool, unassigned, &c, &restricted)
                        .await
                        .status(),
                    StatusCode::FORBIDDEN
                );
                assert_eq!(
                    sqlx::query_scalar::<_, i64>(
                        "SELECT count(*) FROM business_agent_adjustment_intents"
                    )
                    .fetch_one(pool)
                    .await
                    .unwrap(),
                    before
                );
            }
            let prepared = value(forward(core, tool, input.clone(), &c, &allowed).await).await;
            assert!(valid_snapshot(&prepared["document"], kind));
            let replay = value(forward(core, tool, input, &c, &allowed).await).await;
            assert_eq!(prepared["item"]["id"], replay["item"]["id"]);
            let mut limited = allowed.clone();
            if let DataScope::Restricted(dims) = &mut limited.data_scope {
                for (key, id) in [
                    ("customer", f.customer),
                    ("business_unit", f.business_unit),
                    ("warehouse", f.warehouse),
                    ("brand", f.brand),
                ] {
                    dims.insert(key.into(), [id.to_string()].into());
                }
            }
            assert!(permits(
                &prepared["document"],
                &iam_authorization_scope(&limited, &c.required_scope).unwrap(),
                kind
            ));
            for path in [
                "/draftPreview/preview/totalAmount",
                "/draftPreview/preview/referencedOrders/0/customerId",
                "/draftPreview/preview/effects/postsAdjustment",
                "/draftPreview/preview/input/lines/0/amount",
            ] {
                let mut bad = prepared["document"].clone();
                *bad.pointer_mut(path).unwrap() = json!("wrong");
                let bytes = serde_json::to_vec(&bad["draftPreview"]["preview"]).unwrap();
                bad["draftPreview"]["previewHash"] = json!(Sha256::digest(bytes)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>());
                assert!(!valid_snapshot(&bad, kind), "{path}");
            }
            if !create {
                let mut bad = prepared["document"].clone();
                bad["draftPreview"]["preview"]["source"]["lines"][0]["amount"] = json!("99");
                bad["draftPreview"]["previewHash"] = json!(Sha256::digest(
                    serde_json::to_vec(&bad["draftPreview"]["preview"]).unwrap()
                )
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>());
                assert!(!valid_snapshot(&bad, kind));
            }
            let c = context(f.actor, required_capability(approve).unwrap());
            let allowed = grant(&c, f.legal_entity);
            let command = json!({"documentId":prepared["item"]["id"],"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":decision});
            let mut wrong = command.clone();
            wrong["previewHash"] = json!("0".repeat(64));
            assert_eq!(
                forward(core, approve, wrong, &c, &allowed).await.status(),
                StatusCode::CONFLICT
            );
            let result = value(forward(core, approve, command.clone(), &c, &allowed).await).await;
            assert_eq!(result["executed"], decision == "approve" && minimum == 1);
            assert_eq!(
                result["status"],
                if decision == "reject" {
                    "rejected"
                } else if minimum == 1 {
                    "executed"
                } else {
                    "pending"
                }
            );
            let field = drafts::result_field(kind);
            if result["executed"] == true {
                assert_eq!(result[field]["status"], "draft");
                assert_eq!(result[field]["version"], if create { 1 } else { 2 });
                assert!(drafts::valid_result(
                    &result,
                    &prepared["document"],
                    kind,
                    c.trace_id
                ));
                let mut invalid = result.clone();
                invalid[field]["status"] = json!("posted");
                assert!(!drafts::valid_result(
                    &invalid,
                    &prepared["document"],
                    kind,
                    c.trace_id
                ));
            } else {
                assert!(result[field].is_null());
            }
            assert!(result["resourceRefs"].as_array().unwrap().is_empty());
            if let Ok(path) = std::env::var("BUSINESS_ADJUSTMENT_DRAFT_ADAPTER_PROOF") {
                use std::io::Write;
                let mut out = std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(path)
                    .unwrap();
                writeln!(out,"{}",json!({"kind":kind,"prepared":prepared,"approval":result,"decision":decision,"minimumApprovers":minimum})).unwrap();
            }
        }
    }
}
