use super::*;
pub(super) const CORE: &[&str] = &[
    "resourceType",
    "code",
    "name",
    "legalEntityId",
    "businessUnitId",
    "countryCode",
    "functionalCurrency",
    "registrationNumber",
    "address",
    "creditCurrency",
    "creditLimitMinor",
    "paymentTermsDays",
    "expectedVersion",
];
pub(super) const PRODUCT: &[&str] = &[
    "resourceType",
    "code",
    "name",
    "parentCategoryId",
    "categoryId",
    "brandId",
    "baseUomId",
    "productId",
    "unitOfMeasureId",
    "barcode",
    "precisionScale",
    "allowZeroCost",
    "factorToBase",
    "usageScope",
    "expectedVersion",
];
pub(super) fn object<'a>(
    v: &'a Value,
    keys: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, String> {
    let o = v.as_object().ok_or("Master response expected an object")?;
    if o.keys().any(|k| !keys.contains(&k.as_str())) {
        return Err("Master response contains an unexpected field".into());
    }
    Ok(o)
}
pub(super) fn bounded(v: &Value, depth: usize) -> Result<(), String> {
    if depth > 10 {
        return Err("Master response nesting limit".into());
    }
    match v {
        Value::String(s) if s.chars().count() > 4096 => {
            return Err("Master text exceeded limit".into())
        }
        Value::Array(a) => {
            if a.len() > 64 {
                return Err("Master array exceeded limit".into());
            }
            for v in a {
                bounded(v, depth + 1)?;
            }
        }
        Value::Object(o) => {
            if o.len() > 64 {
                return Err("Master object exceeded limit".into());
            }
            for v in o.values() {
                bounded(v, depth + 1)?;
            }
        }
        _ => (),
    }
    Ok(())
}
pub(super) fn uuid(v: &Value) -> bool {
    v.as_str().is_some_and(|s| Uuid::parse_str(s).is_ok())
}
pub(super) fn kind(v: &Value) -> Result<&str, String> {
    let name = v.as_str().ok_or("Missing master resource type")?;
    serde_json::from_value::<master_inputs::MasterKind>(v.clone())
        .map_err(|_| "Unknown master resource type")?;
    Ok(name)
}
pub(super) fn core(kind: &str) -> bool {
    matches!(
        kind,
        "legal_entity" | "business_unit" | "customer" | "supplier" | "warehouse"
    )
}
pub(super) fn flat(v: &Value, keys: &[&str], resource: &str) -> Result<(), String> {
    for (key, value) in object(v, keys)? {
        if value.is_object() || value.is_array() {
            return Err("Nested master field is not allowed".into());
        }
        if value.is_null() {
            continue;
        }
        let valid = if key == "address" {
            resource == "warehouse" && value.as_str().is_some_and(|s| s.chars().count() <= 2000)
        } else if key == "id" || key.ends_with("Id") || key.ends_with("_id") {
            uuid(value)
        } else if matches!(key.as_str(), "version" | "expectedVersion") {
            value.as_i64().is_some_and(|v| v > 0)
        } else if matches!(
            key.as_str(),
            "creditLimitMinor" | "paymentTermsDays" | "precisionScale" | "precision_scale"
        ) {
            value.as_i64().is_some_and(|v| v >= 0)
        } else if matches!(
            key.as_str(),
            "allowZeroCost" | "allow_zero_cost" | "idempotentReplay"
        ) {
            value.is_boolean()
        } else if key == "factorToBase" {
            value
                .as_str()
                .is_some_and(|v| !v.starts_with('-') && valid_decimal_string(v))
        } else if matches!(key.as_str(), "updatedAt" | "updated_at" | "created_at") {
            value
                .as_str()
                .is_some_and(|v| chrono::DateTime::parse_from_rfc3339(v).is_ok())
        } else {
            value.is_string()
        };
        if !valid {
            return Err(format!("Invalid master field: {key}"));
        }
    }
    Ok(())
}
pub(super) fn record(v: &Value) -> Result<(), String> {
    let resource = kind(&v["resourceType"])?;
    let keys = if core(resource) {
        vec![
            "resourceType",
            "id",
            "code",
            "name",
            "status",
            "legalEntityId",
            "legalEntityCode",
            "legalEntityName",
            "businessUnitId",
            "businessUnitCode",
            "businessUnitName",
            "countryCode",
            "functionalCurrency",
            "registrationNumber",
            "address",
            "creditCurrency",
            "creditLimitMinor",
            "paymentTermsDays",
            "version",
            "updatedAt",
        ]
    } else {
        vec![
            "resourceType",
            "id",
            "code",
            "name",
            "status",
            "productId",
            "productCode",
            "productName",
            "categoryId",
            "categoryCode",
            "categoryName",
            "parentCategoryId",
            "parentCategoryCode",
            "parentCategoryName",
            "brandId",
            "brandCode",
            "brandName",
            "unitOfMeasureId",
            "unitOfMeasureCode",
            "unitOfMeasureName",
            "barcode",
            "precisionScale",
            "allowZeroCost",
            "factorToBase",
            "usageScope",
            "version",
            "updatedAt",
        ]
    };
    flat(v, &keys, resource)?;
    if !uuid(&v["id"])
        || v["version"].as_i64().is_none_or(|v| v < 1)
        || !v["code"].is_string()
        || !v["name"].is_string()
        || !matches!(v["status"].as_str(), Some("active" | "disabled"))
    {
        return Err("Invalid master record identity/version/status".into());
    }
    Ok(())
}
pub(super) fn parents(v: &Value) -> Result<(), String> {
    for (key, row) in object(
        v,
        &[
            "legalEntity",
            "businessUnit",
            "category",
            "parentCategory",
            "brand",
            "product",
            "baseUnit",
            "conversionUnit",
        ],
    )? {
        let mut keys = vec![
            "id",
            "code",
            "name",
            "status",
            "version",
            "updated_at",
            "created_at",
        ];
        keys.extend_from_slice(match key.as_str() {
            "legalEntity" => &["country_code", "functional_currency", "registration_number"],
            "businessUnit" => &["legal_entity_id"],
            "category" | "parentCategory" => &["parent_id"],
            "product" => &["category_id", "brand_id", "base_uom_id", "allow_zero_cost"],
            "baseUnit" | "conversionUnit" => &["precision_scale"],
            _ => &[],
        });
        flat(row, &keys, "")?;
        if !uuid(&row["id"]) || row["version"].as_i64().is_none_or(|v| v < 1) {
            return Err("Invalid master parent identity/version".into());
        }
    }
    Ok(())
}
