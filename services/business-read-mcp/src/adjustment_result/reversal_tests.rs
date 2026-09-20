use super::*;

#[test]
fn real_reversal_results_bind_signed_preview_and_effects() {
    let Ok(path) = std::env::var("BUSINESS_ADJUSTMENT_REVERSAL_MCP_FIXTURE_FILE") else {
        return;
    };
    let text = std::fs::read_to_string(path).unwrap();
    let mut states = std::collections::BTreeSet::new();
    let mut count = 0;
    for line in text.lines() {
        let fixture: Value = serde_json::from_str(line).unwrap();
        let p = &fixture["prepared"];
        let mut c = crate::tests::context();
        c.trace_id = p["traceId"].as_str().unwrap().parse().unwrap();
        c.enterprise_user_id = p["document"]["ownerUserId"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let tool = "prepare_operational_adjustment_reversal";
        prepare(tool, p, &c, 131072).unwrap();
        assert!(prepare(tool, p, &c, 1).is_err());
        for key in [
            "previewHash",
            "approvalCommand",
            "rejectionCommand",
            "traceId",
        ] {
            let mut bad = p.clone();
            bad[key] = json!("wrong");
            assert!(prepare(tool, &bad, &c, 131072).is_err(), "{key}");
        }
        for path in [
            "/document",
            "/document/reversalPreview",
            "/document/reversalPreview/preview",
            "/document/reversalPreview/preview/facts/0",
        ] {
            let mut malformed = p.clone();
            *malformed.pointer_mut(path).unwrap() = json!("malformed");
            assert!(prepare(tool, &malformed, &c, 131072).is_err());
        }
        let mut secret = p.clone();
        secret["document"]["reversalPreview"]["preview"]["batch"]["secret"] = json!("not-for-chat");
        assert!(prepare(tool, &secret, &c, 131072).is_err());
        let mut other = c.clone();
        other.enterprise_user_id = Uuid::new_v4();
        assert!(prepare(tool, p, &other, 131072).is_err());
        for path in [
            "/reversalPreview/preview/totalAmount",
            "/reversalPreview/preview/facts/0/fact/amount",
            "/reversalPreview/preview/facts/0/fact/customer_id",
        ] {
            let mut bad = p["document"].clone();
            *bad.pointer_mut(path).unwrap() = json!("0");
            bad["reversalPreview"]["previewHash"] = json!(hex::encode(Sha256::digest(
                serde_json::to_vec(&bad["reversalPreview"]["preview"]).unwrap()
            )));
            assert!(snapshot(&bad, "operational_adjustment_reversal_intent").is_err());
        }
        let v = &fixture["approval"];
        c.trace_id = v["traceId"].as_str().unwrap().parse().unwrap();
        c.approval_document_type = Some("operational_adjustment_reversal_intent".into());
        c.approval_document_id = Some(v["documentId"].as_str().unwrap().parse().unwrap());
        c.approval_expected_version = Some(1);
        c.approval_preview_hash = Some(v["previewHash"].as_str().unwrap().into());
        c.approval_decision = Some(fixture["decision"].as_str().unwrap().into());
        let tool = "approve_operational_adjustment_reversal";
        assert!(approval(tool, v, &c, 131072).is_ok(), "{v}");
        // A separate reviewer is allowed; the signed snapshot retains its original requester.
        c.enterprise_user_id = Uuid::new_v4();
        assert!(approval(tool, v, &c, 131072).is_ok());
        for mode in 0..5 {
            let mut bad = c.clone();
            match mode {
                0 => bad.approval_document_id = Some(Uuid::new_v4()),
                1 => bad.approval_expected_version = Some(2),
                2 => bad.approval_preview_hash = Some("f".repeat(64)),
                3 => bad.approval_document_type = Some("operating_report_snapshot_intent".into()),
                _ => {
                    bad.approval_decision = Some(
                        if fixture["decision"] == "approve" {
                            "reject"
                        } else {
                            "approve"
                        }
                        .into(),
                    )
                }
            }
            assert!(
                approval(tool, v, &bad, 131072).is_err(),
                "context mutation {mode}"
            );
        }
        for key in [
            "id",
            "number",
            "version",
            "status",
            "traceId",
            "idempotentReplay",
        ] {
            let mut bad = v.clone();
            bad["reversedDocument"][key] = json!("wrong");
            assert!(approval(tool, &bad, &c, 131072).is_err(), "{key}");
        }
        for (key, value) in [
            ("executed", json!(!v["executed"].as_bool().unwrap())),
            ("minimumApprovers", json!(0)),
            (
                "resourceRefs",
                json!([{"bizUri":"biz://profit-adjustment/wrong"}]),
            ),
            ("unexpected", json!(true)),
        ] {
            let mut bad = v.clone();
            bad[key] = value;
            assert!(approval(tool, &bad, &c, 131072).is_err(), "{key}");
        }
        if v["executed"] == true {
            for field in ["id", "title", "type", "bizUri"] {
                let mut wrong = v.clone();
                wrong["resourceRefs"][0][field] = json!("wrong");
                assert!(approval(tool, &wrong, &c, 131072).is_err(), "link {field}");
            }
            let mut wrong = v.clone();
            wrong["resourceRefs"] = json!([]);
            assert!(approval(tool, &wrong, &c, 131072).is_err());
        }
        states.insert(v["status"].as_str().unwrap().to_string());
        count += 1;
    }
    assert_eq!(count, 3);
    assert_eq!(
        states,
        ["executed", "pending", "rejected"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
}

#[test]
fn reversal_schema_and_input_are_strict() {
    let all = BusinessReadMcp::all_tools().list_all();
    let prepare = all
        .iter()
        .find(|t| t.name == "prepare_operational_adjustment_reversal")
        .unwrap();
    assert_eq!(
        prepare.input_schema.get("additionalProperties"),
        Some(&json!(false))
    );
    let approve = all
        .iter()
        .find(|t| t.name == "approve_operational_adjustment_reversal")
        .unwrap();
    assert!(approve
        .input_schema
        .get("properties")
        .and_then(Value::as_object)
        .is_none_or(|p| p.is_empty()));
    let input = json!({"batchId":Uuid::new_v4(),"expectedVersion":3,"reason":"重复费用"});
    assert!(reversal::canonical(&input).is_some());
    for (field, value) in [
        ("reason", json!("")),
        ("reason", json!("\u{0000}")),
        ("expectedVersion", json!(0)),
        ("amount", json!("10")),
    ] {
        let mut bad = input.clone();
        bad[field] = value;
        assert!(reversal::canonical(&bad).is_none());
    }
}
#[tokio::test]
async fn invalid_reversal_rejected_before_delegation_consumption() {
    let config =
        crate::tests::production_config(Url::parse("http://127.0.0.1:9/").unwrap(), Uuid::new_v4());
    let mcp = BusinessReadMcp::new(config).unwrap();
    let result = mcp
        .invoke_write(
            "prepare_operational_adjustment_reversal",
            "operational_adjustment_reversal_intent:create",
            json!({"amount":1}),
        )
        .await;
    assert!(result.contains("invalid_input"), "{result}");
}
