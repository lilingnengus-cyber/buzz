use super::*;
use business_core::{
    master_command::MasterCommand,
    master_data::{CoreMasterCommand, SaveCoreMasterData},
    product_master::{ProductMasterCommand, SaveProductMasterData},
};
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Patch {
    pub document_id: Uuid,
    pub resource_type: String,
    pub expected_version: i64,
    pub changes: serde_json::Map<String, Value>,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Approval {
    pub document_id: Uuid,
    pub expected_version: i64,
    pub preview_hash: String,
    pub decision: business_core::document_approval::ApprovalDecision,
}
pub(super) fn family_of_resource(kind: &str) -> Option<&'static str> {
    match kind {
        "legal_entity" | "business_unit" | "customer" | "supplier" | "warehouse" => Some("core"),
        "unit_of_measure" | "product_category" | "brand" | "product" | "sku" | "uom_conversion" => {
            Some("product")
        }
        _ => None,
    }
}
fn fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "legal_entity" => &[
            "name",
            "countryCode",
            "functionalCurrency",
            "registrationNumber",
        ],
        "business_unit" | "unit_of_measure" | "product_category" | "brand" => &["name"],
        "customer" => &[
            "name",
            "creditCurrency",
            "creditLimitMinor",
            "paymentTermsDays",
        ],
        "supplier" => &["name", "paymentTermsDays"],
        "warehouse" => &["name", "address"],
        "product" => &["name", "allowZeroCost"],
        "sku" => &["name", "barcode"],
        "uom_conversion" => &["factorToBase", "usageScope"],
        _ => &[],
    }
}
impl Patch {
    pub(super) fn valid(&self, family: &str) -> bool {
        family_of_resource(&self.resource_type) == Some(family)
            && self.expected_version > 0
            && !self.changes.is_empty()
            && self.changes.iter().all(|(key, value)| {
                fields(&self.resource_type).contains(&key.as_str())
                    && match key.as_str() {
                        "creditLimitMinor" | "paymentTermsDays" => {
                            value.as_i64().is_some_and(|v| v >= 0)
                        }
                        "allowZeroCost" => value.is_boolean(),
                        "registrationNumber" | "address" | "barcode" => {
                            value.is_null() || value.is_string()
                        }
                        _ => value.is_string(),
                    }
            })
    }
    pub(super) fn merge(&self, record: &Value) -> Option<Value> {
        if record["id"] != json!(self.document_id)
            || record["resourceType"] != self.resource_type
            || record["version"] != self.expected_version
        {
            return None;
        }
        let mut input = json!({"resourceType":self.resource_type,"code":record["code"],"name":record["name"],"expectedVersion":self.expected_version});
        for key in fields(&self.resource_type) {
            input[*key] = record.get(*key)?.clone();
        }
        let parents: &[&str] = match self.resource_type.as_str() {
            "business_unit" => &["legalEntityId"],
            "customer" | "supplier" | "warehouse" => &["legalEntityId", "businessUnitId"],
            "unit_of_measure" => &["precisionScale"],
            "product_category" => &["parentCategoryId"],
            "product" => &["categoryId", "brandId"],
            "sku" => &["productId"],
            "uom_conversion" => &["productId", "unitOfMeasureId"],
            _ => &[],
        };
        for key in parents {
            input[*key] = record.get(*key)?.clone();
        }
        if self.resource_type == "product" {
            input["baseUomId"] = record.get("unitOfMeasureId")?.clone();
        }
        if self.resource_type == "uom_conversion" {
            input["code"] = json!("");
            input["name"] = json!("");
        }
        for (key, value) in &self.changes {
            input[key] = value.clone();
        }
        canonical(
            family_of_resource(&self.resource_type)?,
            input,
            Some(self.document_id),
        )
    }
}
pub(super) fn canonical(family: &str, value: Value, id: Option<Uuid>) -> Option<Value> {
    if family_of_resource(value["resourceType"].as_str()?) != Some(family) {
        return None;
    }
    if family == "core" {
        let command: SaveCoreMasterData = serde_json::from_value(value).ok()?;
        if id.is_none() && command.expected_version.is_some() {
            return None;
        }
        let command: CoreMasterCommand = match id {
            None => MasterCommand::Create { command },
            Some(document_id) => MasterCommand::Update {
                document_id,
                command,
            },
        };
        serde_json::to_value(command).ok()
    } else {
        let command: SaveProductMasterData = serde_json::from_value(value).ok()?;
        if id.is_none() && command.expected_version.is_some() {
            return None;
        }
        let command: ProductMasterCommand = match id {
            None => MasterCommand::Create { command },
            Some(document_id) => MasterCommand::Update {
                document_id,
                command,
            },
        };
        serde_json::to_value(command).ok()
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StatusChange {
    document_id: Uuid,
    resource_type: String,
    expected_version: i64,
    status: String,
}
impl StatusChange {
    pub(super) fn command(self, family: &str) -> Option<Value> {
        if family_of_resource(&self.resource_type) != Some(family)
            || self.expected_version < 1
            || !matches!(self.status.as_str(), "active" | "disabled")
        {
            return None;
        }
        Some(
            json!({"operation":"change_status","resourceType":self.resource_type,"documentId":self.document_id,"command":{"expectedVersion":self.expected_version,"status":self.status}}),
        )
    }
}
