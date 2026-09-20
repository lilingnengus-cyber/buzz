use super::*;
const KIND: &str = "operational_adjustment_reversal_intent";
pub(super) async fn verify(
    pool: &sqlx::PgPool,
    core: &CoreClient,
    f: &crate::test_fixture::Fixture,
    order: Uuid,
) {
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,'profit_adjustment:reverse' FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(f.actor).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES('profit_adjustment:reverse','profit_adjustment:reverse',ARRAY['b2_operator'],1,true)").execute(pool).await.unwrap();
    for (decision, minimum) in [("approve", 1), ("reject", 1), ("approve", 2)] {
        sqlx::query("UPDATE business_approval_policies SET min_approvers=$1 WHERE action_code='profit_adjustment:reverse'").bind(minimum as i16).execute(pool).await.unwrap();
        let batch = fixture::draft(
            pool,
            f,
            order,
            &format!("adapter-reverse-{decision}-{minimum}"),
        )
        .await;
        let service = business_core::b4::AdjustmentService::new(
            PgStore::new(pool.clone()),
            "ADJ".into(),
            500,
        );
        let version = business_core::b4::model::VersionCommand {
            expected_version: 1,
        };
        let preview = service
            .allocation_preview(f.actor, batch, &version)
            .await
            .unwrap();
        service
            .post_guarded(
                f.actor,
                Uuid::new_v4(),
                batch,
                &format!("adapter-post-{batch}"),
                &version,
                &preview,
            )
            .await
            .unwrap();
        let tool = "prepare_operational_adjustment_reversal";
        let c = context(f.actor, required_capability(tool).unwrap());
        let allowed = grant(&c, f.legal_entity);
        let input = json!({"batchId":batch,"expectedVersion":3,"reason":"重复费用纠正"});
        assert!(valid(tool, &input));
        for bad in [
            json!({"batchId":batch,"expectedVersion":3,"reason":""}),
            json!({"batchId":batch,"expectedVersion":3,"reason":"x","amount":"1"}),
        ] {
            assert!(!valid(tool, &bad));
        }
        let before: i64 =
            sqlx::query_scalar("SELECT count(*) FROM business_agent_adjustment_intents")
                .fetch_one(pool)
                .await
                .unwrap();
        for dimension in [
            "legal_entity",
            "customer",
            "business_unit",
            "brand",
            "warehouse",
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
                "{dimension}"
            );
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM business_agent_adjustment_intents")
                .fetch_one(pool)
                .await
                .unwrap(),
            before
        );
        let prepared = value(forward(core, tool, input.clone(), &c, &allowed).await).await;
        assert!(valid_snapshot(&prepared["document"], KIND));
        let mut restricted = allowed.clone();
        if let DataScope::Restricted(dims) = &mut restricted.data_scope {
            for (dimension, field) in [
                ("customer", "customer_id"),
                ("business_unit", "business_unit_id"),
                ("brand", "brand_id"),
            ] {
                dims.insert(
                    dimension.into(),
                    [
                        prepared["document"]["reversalPreview"]["preview"]["facts"][0]["fact"]
                            [field]
                            .as_str()
                            .unwrap()
                            .to_owned(),
                    ]
                    .into(),
                );
            }
        }
        let scoped = iam_authorization_scope(&restricted, &c.required_scope).unwrap();
        assert!(permits(&prepared["document"], &scoped, KIND));
        assert!(WRITE_TOOLS.contains(&tool));
        assert!(!is_approval_tool(tool));
        assert!(is_approval_tool("approve_operational_adjustment_reversal"));
        let replay = value(forward(core, tool, input, &c, &allowed).await).await;
        assert_eq!(prepared["item"]["id"], replay["item"]["id"]);
        for (path, changed) in [
            ("/reversalPreview/preview/totalAmount", json!("0")),
            ("/reversalPreview/preview/reason", json!("different")),
            (
                "/reversalPreview/preview/facts/0/fact/amount",
                json!("1.01"),
            ),
            (
                "/reversalPreview/preview/facts/0/fact/source_line_id",
                json!(Uuid::new_v4()),
            ),
            (
                "/reversalPreview/preview/facts/0/fact/customer_id",
                json!(Uuid::new_v4()),
            ),
            ("/reversalPreview/preview/effects/bankRefund", json!(true)),
            (
                "/reversalPreview/preview/targetOrderIds/0",
                json!(Uuid::new_v4()),
            ),
        ] {
            let mut bad = prepared["document"].clone();
            *bad.pointer_mut(path).unwrap() = changed;
            bad["reversalPreview"]["previewHash"] = json!(Sha256::digest(
                serde_json::to_vec(&bad["reversalPreview"]["preview"]).unwrap()
            )
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>());
            assert!(!valid_snapshot(&bad, KIND), "{path}");
        }
        let tool = "approve_operational_adjustment_reversal";
        let c = context(f.actor, required_capability(tool).unwrap());
        let allowed = grant(&c, f.legal_entity);
        let input = json!({"documentId":prepared["item"]["id"],"expectedVersion":1,"previewHash":prepared["previewHash"],"decision":decision});
        let mut wrong = input.clone();
        wrong["previewHash"] = json!("0".repeat(64));
        assert_eq!(
            forward(core, tool, wrong, &c, &allowed).await.status(),
            StatusCode::CONFLICT
        );
        let result = value(forward(core, tool, input, &c, &allowed).await).await;
        let executed = decision == "approve" && minimum == 1;
        assert_eq!(result["executed"], executed);
        assert_eq!(result["preview"], prepared["document"]);
        if executed {
            assert_eq!(result["reversedDocument"]["id"], json!(batch));
            assert_eq!(result["reversedDocument"]["version"], 4);
            let mut bad = result.clone();
            bad["reversedDocument"]["status"] = json!("posted");
            assert!(!reversal::valid_result(
                &bad,
                &prepared["document"],
                c.trace_id
            ));
        } else {
            assert!(result["reversedDocument"].is_null());
        }
        let actual: String =
            sqlx::query_scalar("SELECT status FROM operational_adjustment_batches WHERE id=$1")
                .bind(batch)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(actual, if executed { "reversed" } else { "posted" });
        if let Ok(path) = std::env::var("BUSINESS_ADJUSTMENT_REVERSAL_ADAPTER_PROOF") {
            use std::io::Write;
            let mut output = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap();
            writeln!(output,"{}",json!({"prepared":prepared,"approval":result,"decision":decision,"minimumApprovers":minimum})).unwrap();
        }
    }
}
