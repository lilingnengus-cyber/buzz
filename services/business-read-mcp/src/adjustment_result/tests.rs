use super::*;

#[test]
fn real_adjustment_results_bind_signed_preview_and_effects() {
    let Ok(path) = std::env::var("BUSINESS_ADJUSTMENT_MCP_FIXTURE_FILE") else {
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
        let tool = "prepare_operational_adjustment_post";
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
            "/document/allocationPreview",
            "/document/allocationPreview/preview",
            "/document/allocationPreview/preview/allocations/0",
        ] {
            let mut malformed = p.clone();
            *malformed.pointer_mut(path).unwrap() = json!("malformed");
            assert!(prepare(tool, &malformed, &c, 131072).is_err());
        }
        let mut secret = p.clone();
        secret["document"]["allocationPreview"]["preview"]["batch"]["secret"] =
            json!("not-for-chat");
        assert!(prepare(tool, &secret, &c, 131072).is_err());
        let mut override_currency = p.clone();
        override_currency["document"]["allocationPreview"]["preview"]["allocations"][0]
            ["targets"][0]["currency"] = json!("USD");
        assert!(prepare(tool, &override_currency, &c, 131072).is_err());
        let mut other = c.clone();
        other.enterprise_user_id = Uuid::new_v4();
        assert!(prepare(tool, p, &other, 131072).is_err());
        for path in [
            "/allocationPreview/preview/totalAmount",
            "/allocationPreview/preview/allocations/0/targets/0/amount",
            "/allocationPreview/preview/targets/0/customerId",
        ] {
            let mut bad = p["document"].clone();
            *bad.pointer_mut(path).unwrap() = json!("0");
            bad["allocationPreview"]["previewHash"] = json!(hex::encode(Sha256::digest(
                serde_json::to_vec(&bad["allocationPreview"]["preview"]).unwrap()
            )));
            assert!(snapshot(&bad, "operational_adjustment_post_intent").is_err());
        }
        let v = &fixture["approval"];
        c.trace_id = v["traceId"].as_str().unwrap().parse().unwrap();
        c.approval_document_type = Some("operational_adjustment_post_intent".into());
        c.approval_document_id = Some(v["documentId"].as_str().unwrap().parse().unwrap());
        c.approval_expected_version = Some(1);
        c.approval_preview_hash = Some(v["previewHash"].as_str().unwrap().into());
        c.approval_decision = Some(fixture["decision"].as_str().unwrap().into());
        let tool = "approve_operational_adjustment_post";
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
            bad["postedDocument"][key] = json!("wrong");
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
