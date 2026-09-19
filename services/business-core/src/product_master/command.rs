use super::*;
use crate::master_command::{parent, unchanged, MasterCommand};
use serde_json::Value;
use sqlx::{Postgres, Transaction};

/// Exact product master create, replacement or status command.
pub type ProductMasterCommand = MasterCommand<SaveProductMasterData, ChangeProductMasterStatus>;
impl ProductMasterService {
    /// Preview fixed product master operations without creating business records or intents.
    pub async fn command_preview(
        &self,
        actor: Uuid,
        command: &ProductMasterCommand,
    ) -> Result<Value, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        let result = self.preview_on(&mut tx, actor, command).await?;
        tx.rollback().await?;
        Ok(result)
    }
    /// Save a create/update command only if its preview still matches in the write transaction.
    /// This is a domain consistency guard, not approval authorization. Status execution
    /// remains unavailable until concurrent operational impacts are protected.
    pub async fn save_guarded(
        &self,
        actor: Uuid,
        trace: Uuid,
        key: &str,
        command: &ProductMasterCommand,
        snapshot: &Value,
    ) -> Result<ProductMasterCommandResult, DomainError> {
        match command {
            MasterCommand::Create { command } => {
                self.save_inner((actor, trace), None, key, command, Some(snapshot))
                    .await
            }
            MasterCommand::Update {
                document_id,
                command,
            } => {
                self.save_inner(
                    (actor, trace),
                    Some(*document_id),
                    key,
                    command,
                    Some(snapshot),
                )
                .await
            }
            MasterCommand::ChangeStatus { .. } => Err(DomainError::Invalid(
                "guarded status execution is not available".into(),
            )),
        }
    }

    pub(super) async fn preview_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        command: &ProductMasterCommand,
    ) -> Result<Value, DomainError> {
        self.snapshot(actor, "business_product_master:manage")
            .await?;
        let (kind, id, expected, save, status) = match command {
            MasterCommand::Create { command: input } => {
                let kind = ProductMasterType::from_str(&input.resource_type)?;
                validate(input, kind, false)?;
                applicable(input, kind)?;
                if input.expected_version.is_some() {
                    return Err(DomainError::VersionConflict);
                }
                (kind, None, None, Some(input), None)
            }
            MasterCommand::Update {
                document_id,
                command: input,
            } => {
                let kind = ProductMasterType::from_str(&input.resource_type)?;
                validate(input, kind, true)?;
                applicable(input, kind)?;
                if input.expected_version.is_none_or(|v| v < 1) {
                    return Err(DomainError::VersionConflict);
                }
                (
                    kind,
                    Some(*document_id),
                    input.expected_version,
                    Some(input),
                    None,
                )
            }
            MasterCommand::ChangeStatus {
                resource_type,
                document_id,
                command: input,
            } => {
                if input.expected_version < 1
                    || !matches!(input.status.as_str(), "active" | "disabled")
                {
                    return Err(DomainError::Invalid(
                        "invalid product master status/version".into(),
                    ));
                }
                (
                    ProductMasterType::from_str(resource_type)?,
                    Some(*document_id),
                    Some(input.expected_version),
                    None,
                    Some(input),
                )
            }
        };
        let mut current = if let Some(id) = id {
            write_authority::lock_record(tx, kind, id).await?;
            Some(read_record(tx, kind, id).await?)
        } else {
            None
        };
        if let Some(old) = &current {
            if Some(old.version) != expected {
                return Err(DomainError::VersionConflict);
            }
            if let Some(input) = save {
                immutable_fields(old, input, kind)?;
            }
        }
        let mut parents = serde_json::Map::new();
        let mut category = if kind == ProductMasterType::Product {
            current
                .as_ref()
                .and_then(|v| v.category_id)
                .or_else(|| save.and_then(|v| v.category_id))
        } else {
            None
        };
        let mut brand = if matches!(kind, ProductMasterType::Brand | ProductMasterType::Product) {
            current
                .as_ref()
                .and_then(|v| v.brand_id)
                .or_else(|| save.and_then(|v| v.brand_id))
        } else {
            None
        };
        let mut base_uom = if kind == ProductMasterType::Product {
            current
                .as_ref()
                .and_then(|v| v.unit_of_measure_id)
                .or_else(|| save.and_then(|v| v.base_uom_id))
        } else {
            None
        };
        if matches!(
            kind,
            ProductMasterType::Sku | ProductMasterType::UomConversion
        ) {
            let product_id = current
                .as_ref()
                .and_then(|v| v.product_id)
                .or_else(|| save.and_then(|v| v.product_id))
                .ok_or(DomainError::NotFoundOrForbidden)?;
            let product = parent(tx, "business_products", product_id).await?;
            category = serde_json::from_value(product["category_id"].clone())?;
            brand = serde_json::from_value(product["brand_id"].clone())?;
            base_uom = serde_json::from_value(product["base_uom_id"].clone())?;
            parents.insert("product".into(), product);
        }
        let mut parent_category = if kind == ProductMasterType::ProductCategory {
            current
                .as_ref()
                .and_then(|v| v.parent_category_id)
                .or_else(|| save.and_then(|v| v.parent_category_id))
        } else {
            None
        };
        if let Some(id) = category {
            let category = parent(tx, "business_product_categories", id).await?;
            parent_category = serde_json::from_value(category["parent_id"].clone())?;
            parents.insert("category".into(), category);
        }
        if let Some(id) = parent_category {
            parents.insert(
                "parentCategory".into(),
                parent(tx, "business_product_categories", id).await?,
            );
        }
        if kind != ProductMasterType::Brand {
            if let Some(id) = brand {
                parents.insert("brand".into(), parent(tx, "business_brands", id).await?);
            }
        }
        if let Some(id) = base_uom {
            parents.insert(
                "baseUnit".into(),
                parent(tx, "business_units_of_measure", id).await?,
            );
        }
        if kind == ProductMasterType::UomConversion {
            let unit = current
                .as_ref()
                .and_then(|v| v.unit_of_measure_id)
                .or_else(|| save.and_then(|v| v.unit_of_measure_id))
                .ok_or(DomainError::NotFoundOrForbidden)?;
            parents.insert(
                "conversionUnit".into(),
                parent(tx, "business_units_of_measure", unit).await?,
            );
        }
        if let Some(id) = id {
            current = Some(read_record(tx, kind, id).await?);
        }
        let scope = crate::master_write_authority::snapshot(
            tx,
            actor,
            "business_product_master:manage",
            id.is_none(),
        )
        .await?;
        ensure_brand_scope(&scope, brand)?;
        if let Some(old) = &current {
            ensure_brand_scope(&scope, old.brand_id)?;
        }
        if id.is_none() {
            if let Some(input) = save {
                ensure_inputs_accessible(tx, &scope, kind, input).await?;
            }
        }
        if status.is_some_and(|v| v.status == "active") {
            if let Some(id) = id {
                ensure_enable_dependencies(tx, kind, id).await?;
            }
        }
        if id.is_none() {
            if let Some(input) = save {
                let used: bool = if kind == ProductMasterType::UomConversion {
                    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM business_product_uom_conversions WHERE product_id=$1 AND unit_of_measure_id=$2)").bind(input.product_id).bind(input.unit_of_measure_id).fetch_one(&mut **tx).await?
                } else {
                    let table = write_authority::table(kind);
                    sqlx::query_scalar(AssertSqlSafe(format!(
                        "SELECT EXISTS(SELECT 1 FROM {table} WHERE code=$1)"
                    )))
                    .bind(&input.code)
                    .fetch_one(&mut **tx)
                    .await?
                };
                if used {
                    return Err(DomainError::Invalid(
                        "master code or unit conversion is already in use".into(),
                    ));
                }
            }
        }
        let impacts = if let Some(id) = id {
            load_impacts_on(tx, kind, id).await?
        } else {
            Vec::new()
        };
        let can_execute = status.is_none_or(|v| v.status != "disabled")
            || !impacts.iter().any(|v| v.blocking && v.count > 0);
        let effective = save
            .map(|v| effective_fields(v, kind))
            .unwrap_or_else(|| json!({"status":status.map(|v|&v.status)}));
        Ok(
            json!({"documentType":format!("product_master_{}_intent",command.intent_suffix()),"resourceType":kind.as_str(),"documentId":id,
            "brandId":brand,"current":current,"parents":parents,"command":command,"effectiveFields":effective,"disableImpacts":impacts,"canExecute":can_execute}),
        )
    }
}
async fn read_record(
    tx: &mut Transaction<'_, Postgres>,
    kind: ProductMasterType,
    id: Uuid,
) -> Result<ProductMasterRecord, DomainError> {
    sqlx::query_as("SELECT * FROM product_master_data_maintenance WHERE resource_type=$1 AND id=$2")
        .bind(kind.as_str())
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)
}
fn applicable(v: &SaveProductMasterData, kind: ProductMasterType) -> Result<(), DomainError> {
    if kind == ProductMasterType::UomConversion
        && v.factor_to_base.is_some_and(|factor| {
            factor.normalize().scale() > 8 || factor >= Decimal::from(10_000_000_000_000_000u64)
        })
    {
        return Err(DomainError::Invalid(
            "conversion factor must fit NUMERIC(24,8) without rounding".into(),
        ));
    }
    let allowed: &[&str] = match kind {
        ProductMasterType::UnitOfMeasure => &["precisionScale"],
        ProductMasterType::ProductCategory => &["parentCategoryId"],
        ProductMasterType::Brand => &[],
        ProductMasterType::Product => &["categoryId", "brandId", "baseUomId", "allowZeroCost"],
        ProductMasterType::Sku => &["productId", "barcode"],
        ProductMasterType::UomConversion => {
            &["productId", "unitOfMeasureId", "factorToBase", "usageScope"]
        }
    };
    let value = serde_json::to_value(v)?;
    if value.as_object().is_some_and(|fields| {
        fields.iter().any(|(key, value)| {
            !value.is_null()
                && !["resourceType", "code", "name", "expectedVersion"].contains(&key.as_str())
                && !allowed.contains(&key.as_str())
        })
    }) || (kind == ProductMasterType::UomConversion
        && (!v.code.is_empty() || !v.name.is_empty()))
    {
        return Err(DomainError::Invalid(
            "fields do not apply to this product master type".into(),
        ));
    }
    Ok(())
}
fn immutable_fields(
    old: &ProductMasterRecord,
    v: &SaveProductMasterData,
    kind: ProductMasterType,
) -> Result<(), DomainError> {
    if kind != ProductMasterType::UomConversion {
        unchanged(old.code.as_str(), v.code.as_str())?;
    }
    match kind {
        ProductMasterType::UnitOfMeasure => unchanged(old.precision_scale, v.precision_scale)?,
        ProductMasterType::ProductCategory => {
            unchanged(old.parent_category_id, v.parent_category_id)?
        }
        ProductMasterType::Brand => {}
        ProductMasterType::Product => {
            unchanged(old.category_id, v.category_id)?;
            unchanged(old.brand_id, v.brand_id)?;
            unchanged(old.unit_of_measure_id, v.base_uom_id)?;
        }
        ProductMasterType::Sku => unchanged(old.product_id, v.product_id)?,
        ProductMasterType::UomConversion => {
            unchanged(old.product_id, v.product_id)?;
            unchanged(old.unit_of_measure_id, v.unit_of_measure_id)?;
        }
    }
    Ok(())
}
fn effective_fields(v: &SaveProductMasterData, kind: ProductMasterType) -> Value {
    let mut fields = json!({"code":v.code,"name":v.name.trim()});
    let extra = match kind {
        ProductMasterType::UnitOfMeasure => json!({"precisionScale":v.precision_scale}),
        ProductMasterType::ProductCategory => json!({"parentCategoryId":v.parent_category_id}),
        ProductMasterType::Brand => json!({}),
        ProductMasterType::Product => {
            json!({"categoryId":v.category_id,"brandId":v.brand_id,"baseUomId":v.base_uom_id,"allowZeroCost":v.allow_zero_cost.unwrap_or(false)})
        }
        ProductMasterType::Sku => {
            json!({"productId":v.product_id,"barcode":v.barcode.as_deref().filter(|v|!v.is_empty())})
        }
        ProductMasterType::UomConversion => {
            return json!({"productId":v.product_id,"unitOfMeasureId":v.unit_of_measure_id,"factorToBase":v.factor_to_base,"usageScope":v.usage_scope})
        }
    };
    if let (Some(fields), Some(extra)) = (fields.as_object_mut(), extra.as_object()) {
        fields.extend(extra.clone());
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversion_preview_rejects_silent_rounding_and_overflow() {
        for (factor, valid) in [
            ("0.33333333", true),
            ("1.000000000", true),
            ("0.333333333", false),
            ("10000000000000000", false),
        ] {
            let input:SaveProductMasterData=serde_json::from_value(json!({"resourceType":"uom_conversion","code":"","name":"","productId":Uuid::new_v4(),"unitOfMeasureId":Uuid::new_v4(),"factorToBase":factor,"usageScope":"both"})).unwrap();
            assert_eq!(
                applicable(&input, ProductMasterType::UomConversion).is_ok(),
                valid,
                "{factor}"
            );
        }
    }
}
