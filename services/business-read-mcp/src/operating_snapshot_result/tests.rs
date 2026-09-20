use super::*;
#[test]
fn real_operating_results_bind_signed_preview_owner_and_effects() {
    let Ok(path) = std::env::var("BUSINESS_OPERATING_MCP_FIXTURE_FILE") else {
        return;
    };
    let contents = std::fs::read_to_string(path).unwrap();
    let mut count = 0;
    for line in contents.lines() {
        let fixture: Value = serde_json::from_str(line).unwrap();
        let tool = fixture["tool"].as_str().unwrap();
        let v = &fixture["result"];
        let mut c = crate::tests::context();
        c.trace_id = fixture["traceId"].as_str().unwrap().parse().unwrap();
        if tool.starts_with("prepare_") {
            c.enterprise_user_id = v["document"]["ownerUserId"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap();
            assert!(prepare(tool, v, &c, 65536).is_ok(), "{v}");
            assert!(prepare(tool, v, &c, 1).is_err());
            let mut wrong_owner = c.clone();
            wrong_owner.enterprise_user_id = Uuid::new_v4();
            assert!(prepare(tool, v, &wrong_owner, 65536).is_err());
            for field in ["previewHash", "approvalCommand", "rejectionCommand"] {
                let mut bad = v.clone();
                bad[field] = json!("wrong");
                assert!(prepare(tool, &bad, &c, 65536).is_err());
            }
            for field in ["sourceHash", "periodEndUtc", "ownerUserId"] {
                let mut bad = v.clone();
                bad["document"][field] = json!("wrong");
                assert!(prepare(tool, &bad, &c, 65536).is_err());
            }
        } else {
            c.approval_document_type = Some(family(tool).unwrap().into());
            c.approval_document_id = Some(v["documentId"].as_str().unwrap().parse().unwrap());
            c.approval_expected_version = Some(1);
            c.approval_decision = Some(
                if v["status"] == "rejected" {
                    "reject"
                } else {
                    "approve"
                }
                .into(),
            );
            c.approval_preview_hash = Some(v["previewHash"].as_str().unwrap().into());
            assert!(approval(tool, v, &c, 65536).is_ok(), "{v}");
            for field in [
                "id",
                "ownerUserId",
                "sourceHash",
                "utcOffsetMinutes",
                "created",
                "traceId",
                "generatedAt",
            ] {
                let mut bad = v.clone();
                bad["createdDocument"][field] = json!("wrong");
                assert!(approval(tool, &bad, &c, 65536).is_err());
            }
            let mut bad = v.clone();
            bad["resourceRefs"] = json!([{"bizUri":"biz://management-report/wrong"}]);
            assert!(approval(tool, &bad, &c, 65536).is_err());
            bad = v.clone();
            bad["previewHash"] = json!("f".repeat(64));
            assert!(approval(tool, &bad, &c, 65536).is_err());
            c.approval_document_id = Some(Uuid::new_v4());
            assert!(approval(tool, v, &c, 65536).is_err());
        }
        let preview = if tool.starts_with("prepare_") {
            &v["document"]
        } else {
            &v["preview"]
        };
        if preview["schemaVersion"] == 2
            && preview["metrics"]["unavailableMetrics"]
                .as_object()
                .is_some_and(|o| !o.is_empty())
        {
            for mutation in 0..4 {
                let mut bad = preview.clone();
                match mutation {
                    0 => bad["metrics"]["slaBreached"] = json!(0),
                    1 => {
                        bad["metrics"]["unavailableMetrics"]
                            .as_object_mut()
                            .unwrap()
                            .remove("slaBreached");
                    }
                    2 => bad["dataQualityStatus"] = json!("complete"),
                    _ => bad["scope"]["legalEntityIds"] = json!([Uuid::new_v4()]),
                }
                bad["sourceHash"]=json!(hex::encode(Sha256::digest(serde_json::to_vec(&json!({"cadence":bad["input"]["cadence"],"utcOffsetMinutes":bad["input"]["utcOffsetMinutes"],"periodStartUtc":bad["periodStartUtc"],"periodEndUtc":bad["periodEndUtc"],"periodStart":bad["input"]["periodStart"],"periodEnd":bad["periodEnd"],"currency":bad["input"]["currency"],"scopeHash":bad["scopeHash"],"metrics":bad["metrics"]})).unwrap())));
                assert!(
                    snapshot::validate(&bad, "operating_report_snapshot_intent").is_err(),
                    "mutation {mutation}"
                );
            }
        }
        count += 1;
    }
    assert_eq!(count, 12);
}
