use super::*;
use rust_decimal::Decimal;
fn uuid(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| s.parse::<Uuid>().is_ok_and(|id| !id.is_nil()))
}
fn decimal(v: &Value) -> bool {
    v.as_str().is_some_and(|s| s.parse::<Decimal>().is_ok())
}
pub(super) fn scope_covers(value: &Value, scope: &AuthorizationScope) -> bool {
    [
        ("legalEntityIds", &scope.legal_entity_ids),
        ("customerIds", &scope.customer_ids),
        ("businessUnitIds", &scope.business_unit_ids),
        ("warehouseIds", &scope.warehouse_ids),
        ("brandIds", &scope.brand_ids),
        ("supplierIds", &scope.supplier_ids),
    ]
    .iter()
    .all(|(key, allowed)| {
        value[*key].as_array().is_some_and(|ids| {
            ids.iter().all(|id| {
                uuid(id)
                    && (allowed.is_empty() || id.as_str().is_some_and(|id| allowed.contains(id)))
            })
        })
    })
}
pub(super) fn attributed(v: &Value, scope: &AuthorizationScope) -> bool {
    scope.supplier_ids.is_empty()
        && (scope.brand_ids.is_empty() || v["hasUnattributedBrandTargets"] == false)
        && (v["targetOrderCount"].as_u64().is_some_and(|n| n > 0)
            || (scope.customer_ids.is_empty()
                && scope.business_unit_ids.is_empty()
                && scope.brand_ids.is_empty()
                && scope.warehouse_ids.is_empty()))
}
fn summary(v: &Value) -> bool {
    uuid(&v["id"])
        && uuid(&v["legalEntityId"])
        && v["version"].as_i64().is_some_and(|n| n > 0)
        && v["adjustmentNumber"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
        && matches!(
            v["status"].as_str(),
            Some("draft" | "previewed" | "posted" | "reversed" | "cancelled")
        )
        && v["currency"]
            .as_str()
            .is_some_and(|s| s.len() == 3 && s.bytes().all(|b| b.is_ascii_uppercase()))
        && decimal(&v["totalAmount"])
        && v["lineCount"].as_u64().is_some()
        && v["targetOrderCount"].as_u64().is_some()
        && v["hasUnattributedBrandTargets"].is_boolean()
        && ["createdAt", "updatedAt"].iter().all(|key| {
            v[*key]
                .as_str()
                .is_some_and(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok())
        })
}
pub(super) fn convert(
    v: &Value,
    input: &Value,
    detail: bool,
) -> Option<(Vec<Value>, Option<String>, bool)> {
    let limit = input["limit"].as_u64()?;
    if !detail {
        let items = v["items"].as_array()?;
        let more = v["pagination"]["hasMore"].as_bool()?;
        if items.len() as u64 > limit
            || (more && items.len() as u64 != limit)
            || v["pagination"]["limit"] != limit
            || !items.iter().all(summary)
        {
            return None;
        }
        for item in items {
            if input["legalEntityId"].is_string() && item["legalEntityId"] != input["legalEntityId"]
            {
                return None;
            }
            for (filter, key) in [
                ("managementPeriod", "managementPeriod"),
                ("status", "status"),
            ] {
                if input[filter].is_string() && item[key] != input[filter] {
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
        }
        let next = if more {
            let id = items.last()?["id"].as_str()?;
            if v["pagination"]["nextAfterId"] != id {
                return None;
            }
            Some(id.to_string())
        } else {
            if !v["pagination"]["nextAfterId"].is_null() {
                return None;
            }
            None
        };
        let mut clean = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        for item in items {
            if !ids.insert(item["id"].as_str()?) || item["id"] == input["afterId"] {
                return None;
            }
            let mut row = serde_json::Map::new();
            for key in [
                "id",
                "adjustmentNumber",
                "legalEntityId",
                "currency",
                "managementPeriod",
                "status",
                "version",
                "createdAt",
                "updatedAt",
                "totalAmount",
                "lineCount",
                "targetOrderCount",
                "hasUnattributedBrandTargets",
            ] {
                row.insert(key.into(), item[key].clone());
            }
            clean.push(Value::Object(row));
        }
        return Some((clean, next, more));
    }
    let b = &v["batch"];
    let lines = v["lines"].as_array()?;
    let offset = input["offset"].as_u64()?;
    let total = v["pagination"]["total"].as_u64()?;
    if b["id"] != input["documentId"]
        || b["version"] != v["version"]
        || (input["expectedVersion"].is_number() && v["version"] != input["expectedVersion"])
        || v["pagination"]["offset"] != offset
        || v["pagination"]["limit"] != limit
        || lines.len() as u64 != total.saturating_sub(offset).min(limit)
    {
        return None;
    }
    let mut item = json!({"id":b["id"],"adjustmentNumber":b["adjustment_number"],"legalEntityId":b["legal_entity_id"],"currency":b["currency"],"managementPeriod":b["management_period"],"status":b["status"],"version":v["version"],"createdAt":b["created_at"],"updatedAt":b["updated_at"],"totalAmount":v["totalAmount"],"lineCount":total,"targetOrderCount":v["targetOrderCount"],"hasUnattributedBrandTargets":v["hasUnattributedBrandTargets"]});
    if !summary(&item) {
        return None;
    }
    let mut output = Vec::new();
    for line in lines {
        if !uuid(&line["id"])
            || line["batch_id"] != b["id"]
            || line["currency"] != b["currency"]
            || line["legal_entity_id"] != b["legal_entity_id"]
            || !decimal(&line["amount"])
        {
            return None;
        }
        let mut mapped = json!({"id":line["id"],"lineNumber":line["line_number"],"amount":line["amount"],"currency":line["currency"]});
        for (target, source) in [
            ("metricType", "metric_type"),
            ("businessDate", "business_date"),
            ("allocationBasis", "allocation_basis"),
            ("directSalesOrderId", "direct_sales_order_id"),
            ("customerId", "customer_id"),
            ("skuId", "sku_id"),
            ("brandId", "brand_id"),
            ("salespersonUserId", "salesperson_user_id"),
            ("businessUnitId", "business_unit_id"),
            ("departmentId", "department_id"),
            ("warehouseId", "warehouse_id"),
            ("reasonCode", "reason_code"),
            ("sourceReference", "source_reference"),
            ("businessNote", "business_note"),
        ] {
            mapped[target] = line[source].clone();
        }
        mapped["salesOrderIds"] = line["allocation_scope"]["salesOrderIds"].clone();
        mapped["fixedWeights"] = line["allocation_scope"]["fixedWeights"].clone();
        let mut command = mapped.clone();
        for key in ["id", "lineNumber", "currency"] {
            command.as_object_mut()?.remove(key);
        }
        let _: business_core::b4::model::AdjustmentLineInput =
            serde_json::from_value(command).ok()?;
        if !matches!(
            mapped["metricType"].as_str(),
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
            mapped["allocationBasis"].as_str(),
            Some("direct" | "net_revenue" | "product_cost" | "shipped_quantity" | "fixed_weight")
        ) {
            return None;
        }
        output.push(mapped);
    }
    item["lines"] = json!(output);
    item["lineOffset"] = json!(offset);
    let end = offset.checked_add(lines.len() as u64)?;
    let more = end < total;
    let next = if more {
        if v["pagination"]["nextOffset"] != end {
            return None;
        }
        Some(end.to_string())
    } else {
        if !v["pagination"]["nextOffset"].is_null() {
            return None;
        }
        None
    };
    Some((vec![item], next, more))
}
