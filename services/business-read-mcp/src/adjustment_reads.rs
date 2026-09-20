//! Validate adjustment identity, current versions, decimal amounts and page continuity.
use super::*;
use rust_decimal::Decimal;
pub(super) fn handles(tool: &str) -> bool {
    matches!(
        tool,
        "search_operational_adjustments" | "get_operational_adjustment"
    )
}
fn uuid(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| s.parse::<Uuid>().is_ok_and(|id| !id.is_nil()))
}
fn decimal(v: &Value) -> Option<Decimal> {
    v.as_str()?.parse().ok()
}
pub(super) fn validate(
    tool: &str,
    result: BusinessToolResult<Value>,
    input: &Value,
    c: &DelegationContext,
    max: usize,
) -> Result<BusinessToolResult<Value>, String> {
    let result = validate_result(result, c, max)?;
    if !matches!(result.status, BusinessToolStatus::Ok)
        || result.summary.get("source") != Some(&json!("business-core-adjustments"))
        || result.summary.get("boundary") != Some(&json!("management_only_not_general_ledger"))
        || result.summary.get("detailLinkAvailable") != Some(&json!(true))
        || result.resource_refs.len() != result.items.len()
        || result
            .resource_refs
            .iter()
            .zip(&result.items)
            .any(|(link, item)| {
                link.r#type != "profit_adjustment"
                    || link.id.as_deref() != item["id"].as_str()
                    || link.title != item["adjustmentNumber"].as_str().unwrap_or_default()
                    || link.biz_uri
                        != format!(
                            "biz://profit-adjustment/{}",
                            item["id"].as_str().unwrap_or_default()
                        )
            })
        || c.required_scope != "profit_adjustment:read"
    {
        return Err("Invalid adjustment read envelope".into());
    }
    check(tool, &result, input).ok_or("Invalid adjustment identity, amount or pagination")?;
    Ok(result)
}
fn check(tool: &str, result: &BusinessToolResult<Value>, input: &Value) -> Option<()> {
    let detail = tool == "get_operational_adjustment";
    let page = result.pagination.as_ref()?;
    let limit = input["limit"].as_u64()?;
    if (detail && result.items.len() != 1)
        || (!detail
            && (result.items.len() as u64 > limit
                || (page.has_more && result.items.len() as u64 != limit)))
    {
        return None;
    }
    let mut ids = std::collections::BTreeSet::new();
    for item in &result.items {
        if !uuid(&item["id"])
            || !ids.insert(item["id"].as_str()?)
            || !uuid(&item["legalEntityId"])
            || item["version"].as_i64()? < 1
            || item["adjustmentNumber"].as_str()?.is_empty()
            || decimal(&item["totalAmount"])? < Decimal::ZERO
            || !matches!(
                item["status"].as_str(),
                Some("draft" | "previewed" | "posted" | "reversed" | "cancelled")
            )
            || !item["hasUnattributedBrandTargets"].is_boolean()
            || item["targetOrderCount"].as_u64().is_none()
        {
            return None;
        }
        let currency = item["currency"].as_str()?;
        if currency.len() != 3 || !currency.bytes().all(|b| b.is_ascii_uppercase()) {
            return None;
        }
        for key in ["createdAt", "updatedAt"] {
            chrono::DateTime::parse_from_rfc3339(item[key].as_str()?).ok()?;
        }
        let period = item["managementPeriod"].as_str()?;
        if period.len() != 7 {
            return None;
        }
        chrono::NaiveDate::parse_from_str(&format!("{period}-01"), "%Y-%m-%d").ok()?;
        if !detail {
            for key in ["legalEntityId", "managementPeriod", "status"] {
                if input[key].is_string() && input[key] != item[key] {
                    return None;
                }
            }
            if let Some(number) = input["number"].as_str() {
                if !item["adjustmentNumber"]
                    .as_str()?
                    .to_lowercase()
                    .contains(&number.to_lowercase())
                {
                    return None;
                }
            }
            if item["id"] == input["afterId"] || item.get("lines").is_some() {
                return None;
            }
            item["lineCount"].as_u64()?;
            continue;
        }
        if item["id"] != input["documentId"]
            || (input["expectedVersion"].is_number() && item["version"] != input["expectedVersion"])
        {
            return None;
        }
        let offset = input["offset"].as_u64()?;
        let total = item["lineCount"].as_u64()?;
        let lines = item["lines"].as_array()?;
        if item["lineOffset"] != offset
            || lines.len() as u64 != total.saturating_sub(offset).min(limit)
        {
            return None;
        }
        let mut line_ids = std::collections::BTreeSet::new();
        let mut sum = Decimal::ZERO;
        for line in lines {
            if !uuid(&line["id"])
                || !line_ids.insert(line["id"].as_str()?)
                || line["currency"] != item["currency"]
                || line["lineNumber"].as_u64()? == 0
            {
                return None;
            }
            let amount = decimal(&line["amount"])?;
            if amount <= Decimal::ZERO || amount.round_dp(2) != amount {
                return None;
            }
            sum = sum.checked_add(amount)?;
            for key in [
                "directSalesOrderId",
                "customerId",
                "skuId",
                "brandId",
                "salespersonUserId",
                "businessUnitId",
                "departmentId",
                "warehouseId",
            ] {
                if !line[key].is_null() && !uuid(&line[key]) {
                    return None;
                }
            }
            if !matches!(
                line["metricType"].as_str(),
                Some(
                    "outbound_freight"
                        | "sales_commission"
                        | "platform_fee"
                        | "customer_rebate"
                        | "supplier_rebate"
                        | "other_direct_cost"
                        | "allocated_operating_expense"
                )
            ) || !matches!(
                line["allocationBasis"].as_str(),
                Some(
                    "direct" | "net_revenue" | "product_cost" | "shipped_quantity" | "fixed_weight"
                )
            ) {
                return None;
            }
            chrono::NaiveDate::parse_from_str(line["businessDate"].as_str()?, "%Y-%m-%d").ok()?;
            if line["reasonCode"].as_str()?.is_empty()
                || !line["salesOrderIds"].as_array()?.iter().all(uuid)
            {
                return None;
            }
            for weight in line["fixedWeights"].as_array()? {
                if !uuid(&weight["salesOrderId"]) || decimal(&weight["weight"])? < Decimal::ZERO {
                    return None;
                }
            }
            for key in ["sourceReference", "businessNote"] {
                if !line[key].is_null() && !line[key].is_string() {
                    return None;
                }
            }
        }
        if offset == 0 && lines.len() as u64 == total && sum != decimal(&item["totalAmount"])? {
            return None;
        }
        let next = offset.checked_add(lines.len() as u64)?;
        let more = next < total;
        if page.has_more != more
            || page.next_cursor != if more { Some(next.to_string()) } else { None }
        {
            return None;
        }
    }
    if !detail {
        let expected = if page.has_more {
            Some(result.items.last()?["id"].as_str()?.to_string())
        } else {
            None
        };
        if page.next_cursor != expected {
            return None;
        }
    }
    Some(())
}
#[cfg(test)]
mod tests;
