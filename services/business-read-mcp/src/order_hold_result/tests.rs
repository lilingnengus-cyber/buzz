use super::*;

#[test]
fn real_core_hold_results_bind_effects_confirmation_and_resource_links() {
    let Ok(path) = std::env::var("BUSINESS_ORDER_HOLD_MCP_FIXTURE_FILE") else {
        return;
    };
    let contents = std::fs::read_to_string(path).unwrap();
    let mut count = 0;
    for row in contents.lines() {
        let fixture: Value = serde_json::from_str(row).unwrap();
        let tool = fixture["tool"].as_str().unwrap();
        let v = &fixture["result"];
        let mut c = crate::tests::context();
        c.trace_id = fixture["traceId"].as_str().unwrap().parse().unwrap();
        if tool.starts_with("prepare_") {
            assert!(prepare(tool, v, &c, 65536).is_ok(), "{tool}");
            assert!(prepare(tool, v, &c, 1).is_err());
            for field in ["previewHash", "approvalCommand", "rejectionCommand"] {
                let mut bad = v.clone();
                bad[field] = json!("wrong");
                assert!(prepare(tool, &bad, &c, 65536).is_err());
            }
            let mut bad = v.clone();
            bad["document"]["changesInventoryReservation"] = json!(true);
            assert!(prepare(tool, &bad, &c, 65536).is_err());
        } else {
            c.approval_document_type = Some(family(tool).unwrap().into());
            c.approval_document_id = Some(v["documentId"].as_str().unwrap().parse().unwrap());
            c.approval_expected_version = Some(1);
            c.approval_decision = Some("approve".into());
            c.approval_preview_hash = Some(v["previewHash"].as_str().unwrap().into());
            assert!(approval(tool, v, &c, 65536).is_ok(), "{tool}");
            let mut bad = v.clone();
            bad["createdDocument"]["status"] = json!("other");
            assert!(approval(tool, &bad, &c, 65536).is_err());
            let mut bad = v.clone();
            bad["minimumApprovers"] = json!(2);
            assert!(approval(tool, &bad, &c, 65536).is_err());
            let mut pending = v.clone();
            pending["executed"] = json!(false);
            pending["status"] = json!("pending");
            pending["minimumApprovers"] = json!(2);
            pending["createdDocument"] = Value::Null;
            pending["resourceRefs"] = json!([]);
            assert!(approval(tool, &pending, &c, 65536).is_ok());
            pending["status"] = json!("executed");
            assert!(approval(tool, &pending, &c, 65536).is_err());
            let mut substituted = v.clone();
            let other = Uuid::new_v4();
            substituted["createdDocument"]["id"] = json!(other);
            substituted["resourceRefs"][0]["id"] = json!(other);
            substituted["resourceRefs"][0]["bizUri"] = json!(format!("biz://sales-order/{other}"));
            assert!(approval(tool, &substituted, &c, 65536).is_err());
            let mut rejected = pending.clone();
            rejected["status"] = json!("rejected");
            c.approval_decision = Some("reject".into());
            assert!(approval(tool, &rejected, &c, 65536).is_ok());
            rejected["createdDocument"] = v["createdDocument"].clone();
            assert!(approval(tool, &rejected, &c, 65536).is_err());
            c.approval_decision = Some("approve".into());
            let mut wrong_version = v.clone();
            wrong_version["createdDocument"]["version"] = json!(1);
            assert!(approval(tool, &wrong_version, &c, 65536).is_err());
            let mut wrong_proof = v.clone();
            wrong_proof["previewHash"] = json!("f".repeat(64));
            assert!(approval(tool, &wrong_proof, &c, 65536).is_err());
            let original = c.approval_document_id;
            c.approval_document_id = Some(Uuid::new_v4());
            assert!(approval(tool, v, &c, 65536).is_err());
            c.approval_document_id = original;
        }
        let mut wrong_link = v.clone();
        wrong_link["resourceRefs"][0]["bizUri"] =
            json!(format!("biz://sales-order/{}", Uuid::new_v4()));
        assert!(if tool.starts_with("prepare_") {
            prepare(tool, &wrong_link, &c, 65536)
        } else {
            approval(tool, &wrong_link, &c, 65536)
        }
        .is_err());
        count += 1;
    }
    assert_eq!(count, 4);
}
