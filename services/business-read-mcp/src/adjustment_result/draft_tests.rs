use super::*;
#[test]
fn actual_draft_adapter_responses_bind_signed_state_and_effects() {
    let Ok(path) = std::env::var("BUSINESS_ADJUSTMENT_DRAFT_MCP_FIXTURE_FILE") else {
        return;
    };
    let mut states = std::collections::BTreeSet::new();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let fixture: Value = serde_json::from_str(line).unwrap();
        let kind = fixture["kind"].as_str().unwrap();
        let suffix = kind.strip_suffix("_intent").unwrap();
        let prepare_tool = format!("prepare_{suffix}");
        let approve_tool = format!("approve_{suffix}");
        let p = &fixture["prepared"];
        let v = &fixture["approval"];
        let mut c = crate::tests::context();
        c.enterprise_user_id = p["document"]["ownerUserId"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        c.trace_id = p["traceId"].as_str().unwrap().parse().unwrap();
        prepare(&prepare_tool, p, &c, 131072).unwrap();
        assert_eq!(
            draft_input(&prepare_tool, &p["document"]["input"]).unwrap(),
            p["document"]["input"]
        );
        let prefix = if kind == "operational_adjustment_creation_intent" {
            ""
        } else {
            "/batch"
        };
        for invalid in [json!(1.2), json!("0"), json!("-1"), json!("1.001")] {
            let mut input = p["document"]["input"].clone();
            *input
                .pointer_mut(&format!("{prefix}/lines/0/amount"))
                .unwrap() = invalid;
            assert!(draft_input(&prepare_tool, &input).is_none());
        }
        let mut input = p["document"]["input"].clone();
        input["sql"] = json!("untrusted");
        assert!(draft_input(&prepare_tool, &input).is_none());
        assert!(prepare(&prepare_tool, p, &c, 1).is_err());
        for field in [
            "traceId",
            "approvalCommand",
            "rejectionCommand",
            "previewHash",
        ] {
            let mut bad = p.clone();
            bad[field] = json!("wrong");
            assert!(prepare(&prepare_tool, &bad, &c, 131072).is_err());
        }
        for path in [
            "/document",
            "/document/draftPreview",
            "/document/draftPreview/preview",
            "/document/draftPreview/preview/input",
            "/document/draftPreview/preview/input/lines/0",
        ] {
            let mut bad = p.clone();
            *bad.pointer_mut(path).unwrap() = json!("malformed");
            assert!(prepare(&prepare_tool, &bad, &c, 131072).is_err(), "{path}");
        }
        let mut bad = p.clone();
        bad["document"]["draftPreview"]["preview"]["input"]["lines"][0]["currency"] = json!("USD");
        assert!(prepare(&prepare_tool, &bad, &c, 131072).is_err());
        let mut bad = p.clone();
        bad["document"]["draftPreview"]["preview"]["secret"] = json!("hidden");
        assert!(prepare(&prepare_tool, &bad, &c, 131072).is_err());
        for path in [
            "/draftPreview/preview/totalAmount",
            "/draftPreview/preview/referencedOrders/0/customerId",
            "/draftPreview/preview/input/lines/0/amount",
        ] {
            let mut bad = p["document"].clone();
            *bad.pointer_mut(path).unwrap() = json!("0");
            bad["draftPreview"]["previewHash"] = json!(hex::encode(Sha256::digest(
                serde_json::to_vec(&bad["draftPreview"]["preview"]).unwrap()
            )));
            assert!(snapshot(&bad, kind).is_err());
        }
        c.trace_id = v["traceId"].as_str().unwrap().parse().unwrap();
        c.approval_document_type = Some(kind.into());
        c.approval_document_id = Some(v["documentId"].as_str().unwrap().parse().unwrap());
        c.approval_expected_version = Some(1);
        c.approval_preview_hash = Some(v["previewHash"].as_str().unwrap().into());
        c.approval_decision = Some(fixture["decision"].as_str().unwrap().into());
        approval(&approve_tool, v, &c, 131072).unwrap();
        c.enterprise_user_id = Uuid::new_v4();
        approval(&approve_tool, v, &c, 131072).unwrap();
        for mode in 0..5 {
            let mut wrong = c.clone();
            match mode {
                0 => wrong.approval_document_id = Some(Uuid::new_v4()),
                1 => wrong.approval_expected_version = Some(2),
                2 => wrong.approval_preview_hash = Some("0".repeat(64)),
                3 => {
                    wrong.approval_document_type = Some("operational_adjustment_post_intent".into())
                }
                _ => {
                    wrong.approval_decision = Some(
                        if fixture["decision"] == "approve" {
                            "reject"
                        } else {
                            "approve"
                        }
                        .into(),
                    )
                }
            };
            assert!(approval(&approve_tool, v, &wrong, 131072).is_err());
        }
        let field = drafts::result_field(kind);
        if v["executed"] == true {
            for key in ["status", "version", "traceId"] {
                let mut bad = v.clone();
                bad[field][key] = json!("wrong");
                assert!(approval(&approve_tool, &bad, &c, 131072).is_err());
            }
        } else {
            let mut bad = v.clone();
            bad[field] = json!({"id":Uuid::new_v4(),"status":"draft"});
            assert!(approval(&approve_tool, &bad, &c, 131072).is_err());
        }
        states.insert((kind.to_string(), v["status"].as_str().unwrap().to_string()));
    }
    assert_eq!(states.len(), 6);
}
#[test]
fn draft_tool_schemas_exclude_confirmation_parameters() {
    let all = BusinessReadMcp::all_tools().list_all();
    for suffix in ["creation", "update"] {
        let prepare = format!("prepare_operational_adjustment_{suffix}");
        let approve = format!("approve_operational_adjustment_{suffix}");
        let tool = all.iter().find(|t| t.name.as_ref() == prepare).unwrap();
        assert_eq!(
            tool.input_schema.get("additionalProperties"),
            Some(&json!(false))
        );
        let tool = all.iter().find(|t| t.name.as_ref() == approve).unwrap();
        assert!(tool
            .input_schema
            .get("properties")
            .and_then(Value::as_object)
            .is_none_or(|p| p.is_empty()));
    }
    let schema = serde_json::to_value(schemars::schema_for!(
        crate::adjustment_draft_inputs::CreateAdjustmentBatch
    ))
    .unwrap();
    assert_eq!(
        schema["$defs"]["AdjustmentLine"]["properties"]["amount"]["type"],
        "string"
    );
    assert_eq!(
        schema["$defs"]["AdjustmentLine"]["additionalProperties"],
        false
    );
}
#[tokio::test]
async fn invalid_draft_is_rejected_before_consuming_delegation() {
    let config =
        crate::tests::production_config(Url::parse("http://127.0.0.1:9/").unwrap(), Uuid::new_v4());
    let mcp = BusinessReadMcp::new(config).unwrap();
    let result = mcp
        .invoke_write(
            "prepare_operational_adjustment_creation",
            "operational_adjustment_creation_intent:create",
            json!({"amount":1.2}),
        )
        .await;
    assert!(result.contains("invalid_input"), "{result}");
}
