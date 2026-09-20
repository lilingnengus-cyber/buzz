use super::*;
pub(super) fn validate_write_result(
    tool: &str,
    result: &Value,
    context: &DelegationContext,
    max_payload_bytes: usize,
) -> Result<(), String> {
    if operating_snapshot_result::family(tool).is_some() {
        return operating_snapshot_result::prepare(tool, result, context, max_payload_bytes);
    }
    if report_snapshot_result::family(tool).is_some() {
        return report_snapshot_result::prepare(tool, result, context, max_payload_bytes);
    }
    if order_hold_result::family(tool).is_some() {
        return order_hold_result::prepare(tool, result, context, max_payload_bytes);
    }
    if master_result::family(tool).is_some() {
        return master_result::prepare(tool, result, context, max_payload_bytes);
    }
    let expected_trace_id = context.trace_id.to_string();
    if result.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        || result.get("status").and_then(Value::as_str) != Some("ok")
        || result.get("traceId").and_then(Value::as_str) != Some(expected_trace_id.as_str())
        || result
            .get("item")
            .and_then(|item| item.get("status"))
            .and_then(Value::as_str)
            != Some("draft")
    {
        return Err("Business draft response trace, schema, or status was invalid".into());
    }
    let refs = result
        .get("resourceRefs")
        .and_then(Value::as_array)
        .ok_or_else(|| "Business draft response omitted its resource link".to_string())?;
    let unlinked_count_intent = ((tool == "prepare_inventory_count_creation"
        && result["documentType"] == "inventory_count_creation_intent")
        || (tool == "prepare_crm_creation" && result["documentType"] == "crm_creation_intent"))
        && refs.is_empty()
        && result["item"]["id"]
            .as_str()
            .is_some_and(|id| Uuid::parse_str(id).is_ok())
        && result["previewHash"]
            .as_str()
            .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()));
    if !unlinked_count_intent {
        if refs.len() != 1 {
            return Err("Business draft response must contain exactly one resource link".into());
        }
        let uri = refs[0]
            .get("bizUri")
            .and_then(Value::as_str)
            .ok_or_else(|| "Business draft resource link was invalid".to_string())?;
        let parsed = Url::parse(uri).map_err(|_| "Business draft resource link was invalid")?;
        if !matches!(
            parsed.host_str(),
            Some(
                "sales-order"
                    | "shipment"
                    | "purchase-order"
                    | "goods-receipt"
                    | "sales-return"
                    | "purchase-return"
                    | "customer-receipt"
                    | "supplier-payment"
                    | "inventory-opening"
                    | "inventory-count"
                    | "crm-opportunity"
            )
        ) || !valid_biz_uri(uri)
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path_segments().is_none_or(|mut values| {
                let first = values.next();
                first.is_none() || values.next().is_some()
            })
        {
            return Err("Business draft resource link was not allowlisted".into());
        }
    }
    if serde_json::to_vec(result)
        .map_err(|_| "Business draft response was invalid")?
        .len()
        > max_payload_bytes
    {
        return Err("Business draft response exceeded the payload limit".into());
    }
    Ok(())
}
