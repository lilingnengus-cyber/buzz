use super::*;
#[test]
fn real_report_results_bind_preview_confirmation_and_snapshot_link() {
    let Ok(path) = std::env::var("BUSINESS_REPORT_MCP_FIXTURE_FILE") else {
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
            assert!(prepare(tool, v, &c, 65536).is_ok(), "{v}");
            assert!(prepare(tool, v, &c, 1).is_err());
            for field in ["previewHash", "approvalCommand", "rejectionCommand"] {
                let mut bad = v.clone();
                bad[field] = json!("wrong");
                assert!(prepare(tool, &bad, &c, 65536).is_err());
            }
            let mut bad = v.clone();
            bad["document"]["sourceHash"] = json!("f".repeat(64));
            assert!(prepare(tool, &bad, &c, 65536).is_err());
            bad = v.clone();
            bad["resourceRefs"] = json!([{"type":"management_report","id":Uuid::new_v4(),"title":"wrong","bizUri":"biz://management-report/wrong"}]);
            assert!(prepare(tool, &bad, &c, 65536).is_err());
        } else {
            c.approval_document_type = Some(family(tool).unwrap().into());
            c.approval_document_id = Some(v["documentId"].as_str().unwrap().parse().unwrap());
            c.approval_expected_version = Some(1);
            c.approval_decision = Some("approve".into());
            c.approval_preview_hash = Some(v["previewHash"].as_str().unwrap().into());
            assert!(approval(tool, v, &c, 65536).is_ok(), "{v}");
            for field in ["status", "id", "version", "traceId"] {
                let mut bad = v.clone();
                bad["createdDocument"][field] = json!("invalid");
                assert!(approval(tool, &bad, &c, 65536).is_err());
            }
            let mut bad = v.clone();
            bad["resourceRefs"][0]["bizUri"] =
                json!(format!("biz://management-report/{}", Uuid::new_v4()));
            assert!(approval(tool, &bad, &c, 65536).is_err());
            bad = v.clone();
            bad["previewHash"] = json!("f".repeat(64));
            assert!(approval(tool, &bad, &c, 65536).is_err());
            let mut pending = v.clone();
            pending["executed"] = json!(false);
            pending["status"] = json!("pending");
            pending["minimumApprovers"] = json!(2);
            pending["createdDocument"] = Value::Null;
            pending["resourceRefs"] = json!([]);
            assert!(approval(tool, &pending, &c, 65536).is_ok());
            pending["status"] = json!("rejected");
            c.approval_decision = Some("reject".into());
            assert!(approval(tool, &pending, &c, 65536).is_ok());
            pending["createdDocument"] = v["createdDocument"].clone();
            assert!(approval(tool, &pending, &c, 65536).is_err());
            c.approval_decision = Some("approve".into());
            c.approval_document_id = Some(Uuid::new_v4());
            assert!(approval(tool, v, &c, 65536).is_err());
        }
        count += 1;
    }
    assert_eq!(count, 6);
}
