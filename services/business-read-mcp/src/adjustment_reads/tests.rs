use super::*;
#[test]
fn real_adjustment_read_results_bind_filters_versions_and_pages() {
    let Ok(path) = std::env::var("BUSINESS_ADJUSTMENT_READ_PROOF") else {
        return;
    };
    let proof: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut c = crate::tests::context();
    c.trace_id = proof["traceId"].as_str().unwrap().parse().unwrap();
    c.required_scope = "profit_adjustment:read".into();
    for (tool, key, input) in [
        (
            "search_operational_adjustments",
            "search",
            json!({"limit":1}),
        ),
        (
            "get_operational_adjustment",
            "detail",
            json!({"documentId":proof["detail"]["items"][0]["id"],"limit":10,"offset":0}),
        ),
    ] {
        let result: BusinessToolResult<Value> = serde_json::from_value(proof[key].clone()).unwrap();
        assert!(validate(tool, result.clone(), &input, &c, 131072).is_ok());
        assert!(validate(tool, result.clone(), &input, &c, 1).is_err());
        let mut wrong = result.clone();
        wrong.pagination.as_mut().unwrap().next_cursor = Some("wrong".into());
        assert!(validate(tool, wrong, &input, &c, 131072).is_err());
        if key == "detail" {
            let mut wrong_input = input.clone();
            wrong_input["documentId"] = json!(Uuid::new_v4());
            assert!(validate(tool, result.clone(), &wrong_input, &c, 131072).is_err());
            for path in [
                "/lines/0/amount",
                "/lines/0/currency",
                "/lines/0/businessDate",
                "/version",
            ] {
                let mut wrong = result.clone();
                *wrong.items[0].pointer_mut(path).unwrap() = json!("wrong");
                assert!(validate(tool, wrong, &input, &c, 131072).is_err());
            }
        }
    }
}
