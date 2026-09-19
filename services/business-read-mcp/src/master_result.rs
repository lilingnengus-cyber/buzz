//! Only fixed master shapes may carry bounded maintenance fields such as address.
use super::*;
use sha2::{Digest, Sha256};
mod fields;
use fields::*;

pub(super) fn family(tool: &str) -> Option<String> {
    for prefix in ["prepare_", "approve_"] {
        if let Some(name) = tool.strip_prefix(prefix) {
            if matches!(
                name,
                "core_master_status"
                    | "product_master_status"
                    | "core_master_creation"
                    | "core_master_update"
                    | "product_master_creation"
                    | "product_master_update"
            ) {
                return Some(format!("{name}_intent"));
            }
        }
    }
    None
}
fn limit(v: &Value, max: usize) -> Result<(), String> {
    if serde_json::to_vec(v)
        .map_err(|_| "Invalid master payload")?
        .len()
        > max
    {
        return Err("Master response exceeded payload limit".into());
    }
    bounded(v, 0)
}
pub(super) fn read(
    tool: &str,
    result: BusinessToolResult<Value>,
    context: &DelegationContext,
    max: usize,
) -> Result<BusinessToolResult<Value>, String> {
    if !matches!(
        tool,
        "get_business_master_record" | "get_business_product_master_record"
    ) {
        return crm_result::validate(tool, result, context, max);
    }
    limit(
        &serde_json::to_value(&result).map_err(|_| "Invalid master result")?,
        max,
    )?;
    if result.items.len() != 1 || !result.evidence.is_empty() || !result.warnings.is_empty() {
        return Err("Master detail must contain exactly one record".into());
    }
    record(&result.items[0])?;
    if tool == "get_business_product_master_record" && core(kind(&result.items[0]["resourceType"])?)
    {
        return Err("Product reader received a core record".into());
    }
    references(
        &serde_json::to_value(&result.resource_refs).map_err(|_| "Invalid references")?,
        &result.items[0],
    )?;
    object(
        &serde_json::to_value(&result.summary).map_err(|_| "Invalid master summary")?,
        &["source"],
    )?;
    let mut checked = result.clone();
    if let Some(row) = checked.items[0].as_object_mut() {
        row.remove("address");
    }
    validate_result(checked, context, max)?;
    Ok(result)
}
pub(super) fn prepare(
    tool: &str,
    result: &Value,
    context: &DelegationContext,
    max: usize,
) -> Result<(), String> {
    let expected = family(tool).ok_or("Unknown master tool")?;
    limit(result, max)?;
    object(
        result,
        &[
            "item",
            "document",
            "previewHash",
            "approvalCommand",
            "rejectionCommand",
            "traceId",
            "schemaVersion",
            "status",
            "documentType",
            "resourceRefs",
        ],
    )?;
    object(&result["item"], &["id", "version", "status"])?;
    let id = result["item"]["id"]
        .as_str()
        .ok_or("Missing master intent ID")?;
    if !uuid(&result["item"]["id"])
        || result["item"]["version"] != 1
        || result["item"]["status"] != "draft"
        || result["schemaVersion"] != 1
        || result["status"] != "ok"
        || result["traceId"] != json!(context.trace_id)
        || result["documentType"] != expected
    {
        return Err("Invalid master preparation envelope".into());
    }
    let document = &result["document"];
    preview(document, &expected)?;
    let hash = hex::encode(Sha256::digest(
        serde_json::to_vec(document).map_err(|_| "Invalid master preview")?,
    ));
    if result["previewHash"] != hash
        || result["approvalCommand"]
            != format!("确认 {} {id} v1 {hash}", expected.replace('_', "-"))
        || result["rejectionCommand"]
            != format!("拒绝 {} {id} v1 {hash}", expected.replace('_', "-"))
    {
        return Err("Master confirmation does not bind its preview".into());
    }
    // An unexecuted creation intent is not a master record.
    if result["resourceRefs"] != json!([]) {
        return Err("Unsupported master resource reference".into());
    }
    Ok(())
}
fn preview(v: &Value, expected: &str) -> Result<(), String> {
    object(
        v,
        &[
            "documentType",
            "resourceType",
            "documentId",
            "legalEntityId",
            "businessUnitId",
            "brandId",
            "current",
            "parents",
            "command",
            "effectiveFields",
            "disableImpacts",
            "canExecute",
        ],
    )?;
    let resource = kind(&v["resourceType"])?;
    if v["documentType"] != expected
        || core(resource) != expected.starts_with("core_")
        || (!expected.ends_with("status_intent") && v["canExecute"] != true)
        || !v["canExecute"].is_boolean()
    {
        return Err("Master preview family mismatch".into());
    }
    for key in ["documentId", "legalEntityId", "businessUnitId", "brandId"] {
        if !v[key].is_null() && !uuid(&v[key]) {
            return Err("Invalid master dimension".into());
        }
    }
    let creation = expected.ends_with("creation_intent");
    let status = expected.ends_with("status_intent");
    let command = &v["command"];
    object(
        command,
        if creation {
            &["operation", "command"]
        } else if status {
            &["operation", "resourceType", "documentId", "command"]
        } else {
            &["operation", "documentId", "command"]
        },
    )?;
    let fields = &command["command"];
    if (if status {
        &command["resourceType"]
    } else {
        &fields["resourceType"]
    }) != resource
        || command["operation"]
            != if creation {
                "create"
            } else if status {
                "change_status"
            } else {
                "update"
            }
    {
        return Err("Master preview operation mismatch".into());
    }
    if status {
        object(fields, &["status", "expectedVersion"])?;
        if !matches!(fields["status"].as_str(), Some("active" | "disabled"))
            || fields["expectedVersion"].as_i64().is_none_or(|v| v < 1)
        {
            return Err("Invalid status command".into());
        }
    } else {
        flat(
            fields,
            if core(resource) { CORE } else { PRODUCT },
            resource,
        )?;
    }
    if creation {
        if !v["current"].is_null()
            || !v["documentId"].is_null()
            || !fields["expectedVersion"].is_null()
        {
            return Err("Creation preview contains an existing target".into());
        }
    } else {
        record(&v["current"])?;
        if v["current"]["resourceType"] != resource
            || v["current"]["id"] != v["documentId"]
            || command["documentId"] != v["documentId"]
            || fields["expectedVersion"] != v["current"]["version"]
        {
            return Err("Master update target or version mismatch".into());
        }
    }
    parents(&v["parents"])?;
    if status {
        object(&v["effectiveFields"], &["status"])?;
        if v["effectiveFields"]["status"] != fields["status"] {
            return Err("Status effect mismatch".into());
        }
    } else {
        flat(
            &v["effectiveFields"],
            if core(resource) { CORE } else { PRODUCT },
            resource,
        )?;
    }
    let impacts = v["disableImpacts"]
        .as_array()
        .ok_or("Missing master impact list")?;
    for impact in impacts {
        object(impact, &["code", "label", "count", "blocking"])?;
        if !impact["code"].is_string()
            || !impact["label"].is_string()
            || impact["count"].as_i64().is_none_or(|v| v < 0)
            || !impact["blocking"].is_boolean()
        {
            return Err("Invalid master impact".into());
        }
    }
    if status {
        let blocked = impacts
            .iter()
            .any(|i| i["blocking"] == true && i["count"].as_i64().is_some_and(|v| v > 0));
        if v["canExecute"] != json!(fields["status"] != "disabled" || !blocked) {
            return Err("Status readiness mismatch".into());
        }
    }
    Ok(())
}
pub(super) fn approval(
    tool: &str,
    v: &Value,
    c: &DelegationContext,
    max: usize,
) -> Result<(), String> {
    let expected = family(tool).ok_or("Unknown master tool")?;
    limit(v, max)?;
    object(
        v,
        &[
            "documentId",
            "documentType",
            "requestId",
            "status",
            "executed",
            "createdDocument",
            "approvalCount",
            "minimumApprovers",
            "traceId",
            "resourceRefs",
        ],
    )?;
    if v["documentId"] != json!(c.approval_document_id)
        || v["documentType"] != expected
        || c.approval_document_type.as_deref() != Some(&expected)
        || v["traceId"] != json!(c.trace_id)
        || !uuid(&v["requestId"])
    {
        return Err("Master approval response binding mismatch".into());
    }
    let count = v["approvalCount"]
        .as_i64()
        .filter(|v| *v >= 0)
        .ok_or("Invalid master approval count")?;
    let minimum = v["minimumApprovers"]
        .as_i64()
        .filter(|v| (1..=10).contains(v))
        .ok_or("Invalid master approval threshold")?;
    match (
        v["executed"].as_bool(),
        v["status"].as_str(),
        c.approval_decision.as_deref(),
    ) {
        (Some(true), Some("executed"), Some("approve")) if count >= minimum => {
            let row = &v["createdDocument"];
            references(&v["resourceRefs"], row)?;
            flat(
                row,
                &[
                    "id",
                    "resourceType",
                    "code",
                    "status",
                    "version",
                    "traceId",
                    "idempotentReplay",
                ],
                "",
            )?;
            let resource = kind(&row["resourceType"])?;
            if core(resource) != expected.starts_with("core_")
                || !uuid(&row["id"])
                || row["traceId"] != json!(c.trace_id)
                || !row["idempotentReplay"].is_boolean()
                || !row["code"].is_string()
                || !matches!(row["status"].as_str(), Some("active" | "disabled"))
                || row["version"].as_i64().is_none_or(|v| v < 1)
                || (expected.ends_with("creation_intent")
                    && (row["version"] != 1 || row["status"] != "active"))
            {
                return Err("Invalid executed master record".into());
            }
        }
        (Some(false), Some("pending"), Some("approve"))
            if count < minimum
                && v["createdDocument"].is_null()
                && v["resourceRefs"] == json!([]) => {}
        (Some(false), Some("rejected"), Some("reject"))
            if v["createdDocument"].is_null() && v["resourceRefs"] == json!([]) => {}
        _ => return Err("Inconsistent master approval outcome".into()),
    }
    Ok(())
}

fn references(refs: &Value, row: &Value) -> Result<(), String> {
    let kind = kind(&row["resourceType"])?;
    let id = row["id"].as_str().ok_or("Missing record ID")?;
    let expected = json!([{"type":"master_data","id":id,"title":row["code"],
        "bizUri":format!("biz://master-data/{kind}/{id}")}]);
    if *refs != expected {
        return Err("Master reference does not match the record".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_adapter_response_corpus() {
        let Ok(path) = std::env::var("BUSINESS_MASTER_MCP_FIXTURE_FILE") else {
            return;
        };
        let corpus = std::fs::read_to_string(path).unwrap();
        let mut count = 0;
        for line in corpus.lines() {
            let row: Value = serde_json::from_str(line).unwrap();
            let tool = row["tool"].as_str().unwrap();
            let result = &row["result"];
            let mut context = crate::tests::context();
            context.trace_id = serde_json::from_value(row["traceId"].clone()).unwrap();
            let outcome = if tool.starts_with("prepare_") {
                prepare(tool, result, &context, 262144)
            } else if tool.starts_with("approve_") {
                context.approval_document_id =
                    serde_json::from_value(result["documentId"].clone()).unwrap();
                context.approval_document_type = family(tool);
                context.approval_decision = Some("approve".into());
                approval(tool, result, &context, 262144)
            } else {
                read(
                    tool,
                    serde_json::from_value(result.clone()).unwrap(),
                    &context,
                    262144,
                )
                .map(|_| ())
            };
            assert!(outcome.is_ok(), "{tool}: {outcome:?}");
            if tool.starts_with("prepare_") {
                let mut tampered = result.clone();
                tampered["document"]["effectiveFields"]["name"] = json!("Altered after preview");
                assert!(prepare(tool, &tampered, &context, 262144).is_err());
                let mut secret = result.clone();
                secret["document"]["command"]["command"]["password"] = json!("forbidden");
                assert!(prepare(tool, &secret, &context, 262144).is_err());
            }
            count += 1;
        }
        assert!(count >= 47, "Incomplete isolated corpus: {count}");
    }

    #[test]
    fn executed_result_requires_signed_binding_and_consistent_outcome() {
        let mut context = crate::tests::context();
        context.approval_document_id = Some(Uuid::new_v4());
        context.approval_document_type = Some("core_master_creation_intent".into());
        context.approval_decision = Some("approve".into());
        let mut result = json!({"documentId":context.approval_document_id,
            "documentType":"core_master_creation_intent","requestId":Uuid::new_v4(),
            "status":"executed","executed":true,"approvalCount":1,"minimumApprovers":1,
            "traceId":context.trace_id,"resourceRefs":[],"createdDocument":{
                "id":Uuid::new_v4(),"resourceType":"warehouse","code":"WH",
                "status":"active","version":1,"traceId":context.trace_id,"idempotentReplay":false}});
        let id = result["createdDocument"]["id"].as_str().unwrap();
        result["resourceRefs"] = json!([{"type":"master_data","id":id,"title":"WH","bizUri":format!("biz://master-data/warehouse/{id}")}]);
        let check =
            |value: &Value| approval("approve_core_master_creation", value, &context, 65536);
        assert!(check(&result).is_ok());
        for (field, value) in [
            ("traceId", json!(Uuid::new_v4())),
            ("documentId", json!(Uuid::new_v4())),
            ("approvalCount", json!(0)),
            ("executed", json!(false)),
            ("status", json!("pending")),
        ] {
            let mut invalid = result.clone();
            invalid[field] = value;
            assert!(check(&invalid).is_err(), "{field}");
        }
        let mut wrong_link = result.clone();
        wrong_link["resourceRefs"][0]["bizUri"] =
            json!(format!("biz://master-data/customer/{}", Uuid::new_v4()));
        assert!(check(&wrong_link).is_err());
        let mut invalid = result.clone();
        invalid["createdDocument"]["accessToken"] = json!("not-allowed");
        assert!(check(&invalid).is_err());
    }

    #[test]
    fn maintenance_fields_do_not_allow_nested_or_unbounded_data() {
        let record = json!({"resourceType":"warehouse","id":Uuid::new_v4(),
            "code":"WH","name":"Warehouse","status":"active","version":1,"address":"杭州"});
        assert!(fields::record(&record).is_ok());
        for value in [json!({"accessToken":"secret"}), json!("a".repeat(2001))] {
            let mut invalid = record.clone();
            invalid["address"] = value;
            assert!(fields::record(&invalid).is_err());
        }
        let mut invalid = record.clone();
        invalid["resourceType"] = json!("customer");
        assert!(fields::record(&invalid).is_err());
        assert!(validate_business_value(&record).is_err());
    }
}
