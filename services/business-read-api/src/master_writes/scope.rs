use super::*;
use std::collections::BTreeSet;
// A restricted dimension with no corresponding object is not unrestricted.
// In particular, an existing-customer grant cannot authorize a new customer ID.
fn matches(value: Option<&Value>, allowed: &BTreeSet<String>) -> bool {
    if allowed.is_empty() {
        return true;
    }
    value
        .and_then(Value::as_str)
        .is_some_and(|id| Uuid::parse_str(id).is_ok() && allowed.contains(id))
}
pub(super) fn record(record: &Value, scope: &AuthorizationScope) -> bool {
    let kind = record["resourceType"].as_str().unwrap_or("");
    let legal = if input::family_of_resource(kind) == Some("core") {
        record.get("legalEntityId")
    } else {
        None
    };
    let unit = if input::family_of_resource(kind) == Some("core") {
        record.get("businessUnitId")
    } else {
        None
    };
    let brand = if input::family_of_resource(kind) == Some("product") {
        record.get("brandId")
    } else {
        None
    };
    input::family_of_resource(kind).is_some()
        && matches(legal, &scope.legal_entity_ids)
        && matches(unit, &scope.business_unit_ids)
        && matches(brand, &scope.brand_ids)
        && matches(
            (kind == "customer").then(|| &record["id"]),
            &scope.customer_ids,
        )
        && matches(
            (kind == "supplier").then(|| &record["id"]),
            &scope.supplier_ids,
        )
        && matches(
            (kind == "warehouse").then(|| &record["id"]),
            &scope.warehouse_ids,
        )
}
pub(super) fn preview(snapshot: &Value, scope: &AuthorizationScope, kind: &str) -> bool {
    let command = &snapshot["command"];
    let resource = &snapshot["resourceType"];
    if snapshot["documentType"] != kind || command["command"]["resourceType"] != *resource {
        return false;
    }
    let creation = kind.ends_with("creation_intent");
    if creation {
        if command["operation"] != "create"
            || !snapshot["current"].is_null()
            || !snapshot["documentId"].is_null()
        {
            return false;
        }
        record(
            &json!({"resourceType":resource,"id":null,"legalEntityId":snapshot["legalEntityId"],"businessUnitId":snapshot["businessUnitId"],"brandId":snapshot["brandId"]}),
            scope,
        )
    } else {
        command["operation"] == "update"
            && snapshot["current"]["resourceType"] == *resource
            && snapshot["current"]["id"] == snapshot["documentId"]
            && command["documentId"] == snapshot["documentId"]
            && snapshot["current"]["version"] == command["command"]["expectedVersion"]
            && ["legalEntityId", "businessUnitId", "brandId"]
                .iter()
                .all(|key| snapshot[*key] == snapshot["current"][*key])
            && record(&snapshot["current"], scope)
    }
}

pub(super) fn delegation(grant: &EffectiveGrant, required: &str) -> Option<AuthorizationScope> {
    if let DataScope::Restricted(dimensions) = &grant.data_scope {
        let mut seen = BTreeSet::new();
        for key in dimensions.keys() {
            let canonical = match key.as_str() {
                "legal_entity" | "legal_entity_id" | "legalEntityIds" => "legal_entity",
                "business_unit" | "business_unit_id" | "businessUnitIds" => "business_unit",
                "customer" | "customer_id" | "customerIds" => "customer",
                "supplier" | "supplier_id" | "supplierIds" => "supplier",
                "warehouse" | "warehouse_id" | "warehouseIds" => "warehouse",
                "brand" | "brand_id" | "brandIds" => "brand",
                _ => return None,
            };
            // Two aliases cannot be silently unioned into a wider write grant.
            if !seen.insert(canonical) {
                return None;
            }
        }
    }
    iam_authorization_scope(grant, required)
}
