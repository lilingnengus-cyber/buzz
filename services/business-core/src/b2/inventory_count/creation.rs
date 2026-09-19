//! Shared locked creation plan for count previews and execution.
use super::*;

pub(super) struct CreationPlan {
    pub(super) business_unit_id: Uuid,
    pub(super) brands: BTreeMap<Uuid, Option<Uuid>>,
    pub(super) balances: Vec<sqlx::postgres::PgRow>,
    pub(super) snapshot: Value,
}

pub(super) async fn plan(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope: &crate::model::AuthorizationSnapshot,
    input: &CreateInventoryCount,
) -> Result<CreationPlan, DomainError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(input.warehouse_id.to_string())
        .execute(&mut **tx)
        .await?;
    let functional_currency: String = sqlx::query_scalar(
        "SELECT functional_currency::text FROM business_legal_entities WHERE id=$1 FOR SHARE",
    )
    .bind(input.legal_entity_id)
    .fetch_one(&mut **tx)
    .await?;
    if input.currency != functional_currency {
        return Err(DomainError::Invalid(
            "inventory count currency must match the legal entity functional currency".into(),
        ));
    }
    let business_unit_id: Uuid = sqlx::query_scalar(
            "SELECT business_unit_id FROM business_warehouses WHERE id=$1 AND legal_entity_id=$2 AND status='active' FOR SHARE",
        )
        .bind(input.warehouse_id)
        .bind(input.legal_entity_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
    if !scope.scopes.business_unit_ids.contains(&business_unit_id) {
        return Err(DomainError::NotFoundOrForbidden);
    }
    let brand_rows = sqlx::query(
            "SELECT s.id,p.brand_id FROM business_skus s JOIN business_products p ON p.id=s.product_id WHERE s.id=ANY($1) ORDER BY p.id,s.id FOR SHARE OF s,p",
        ).bind(&input.sku_ids).fetch_all(&mut **tx).await?;
    let brands: BTreeMap<Uuid, Option<Uuid>> = brand_rows
        .iter()
        .map(|row| (row.get("id"), row.get("brand_id")))
        .collect();
    if brands.len() != input.sku_ids.len()
        || brands
            .values()
            .flatten()
            .any(|brand| !scope.scopes.brand_ids.contains(brand))
    {
        return Err(DomainError::NotFoundOrForbidden);
    }
    let balances=sqlx::query("SELECT sku_id,on_hand_quantity,reserved_quantity,quarantined_quantity,inventory_value,average_unit_cost,last_movement_id,version FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=ANY($3) ORDER BY sku_id FOR UPDATE").bind(input.legal_entity_id).bind(input.warehouse_id).bind(&input.sku_ids).fetch_all(&mut **tx).await?;
    if balances.len() != input.sku_ids.len() {
        return Err(DomainError::NotFoundOrForbidden);
    }
    let overlap:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM inventory_count_tasks t JOIN inventory_count_lines l ON l.inventory_count_id=t.id WHERE t.status IN ('counting','counted') AND t.legal_entity_id=$1 AND t.warehouse_id=$2 AND l.sku_id=ANY($3))").bind(input.legal_entity_id).bind(input.warehouse_id).bind(&input.sku_ids).fetch_one(&mut **tx).await?;
    if overlap {
        return Err(DomainError::Invalid(
            "inventory count scope is already frozen".into(),
        ));
    }

    // Lock the authorization revision after resource waits. Scope changes
    // cannot commit between this comparison and creation's transaction commit.
    let revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM business_authorization_revision WHERE singleton FOR SHARE",
    )
    .fetch_one(&mut **tx)
    .await?;
    if revision != scope.scope_version {
        return Err(DomainError::NotFoundOrForbidden);
    }
    let fresh = PgStore::snapshot_on(&mut *tx, scope.enterprise_user_id)
        .await
        .map_err(|_| DomainError::NotFoundOrForbidden)?;
    if !fresh.permission_keys.contains("inventory_opening:create")
        || !fresh
            .scopes
            .legal_entity_ids
            .contains(&input.legal_entity_id)
        || !fresh.scopes.warehouse_ids.contains(&input.warehouse_id)
        || !fresh.scopes.business_unit_ids.contains(&business_unit_id)
        || brands
            .values()
            .flatten()
            .any(|id| !fresh.scopes.brand_ids.contains(id))
    {
        return Err(DomainError::NotFoundOrForbidden);
    }
    let lines: Vec<Value> = balances.iter().map(|row| json!({
        "skuId":row.get::<Uuid,_>("sku_id"),
        "brandId":brands.get(&row.get::<Uuid,_>("sku_id")).copied().flatten(),
        "onHandQuantity":row.get::<Decimal,_>("on_hand_quantity").to_string(),
        "reservedQuantity":row.get::<Decimal,_>("reserved_quantity").to_string(),
        "quarantinedQuantity":row.get::<Decimal,_>("quarantined_quantity").to_string(),
        "inventoryValue":row.get::<Decimal,_>("inventory_value").to_string(),
        "averageUnitCost":row.get::<Option<Decimal>,_>("average_unit_cost").map(|value|value.to_string()),
        "lastMovementId":row.get::<Option<Uuid>,_>("last_movement_id"),
        "version":row.get::<i64,_>("version"),
    })).collect();
    let snapshot = json!({"command":input,"businessUnitId":business_unit_id,"lines":lines,
        "effect":"freeze_selected_inventory_until_count_posted_or_cancelled"});
    Ok(CreationPlan {
        business_unit_id,
        brands,
        balances,
        snapshot,
    })
}

impl InventoryCountService {
    /// Preview the exact inventory scope that count creation will freeze, without persisting it.
    pub async fn creation_preview(
        &self,
        actor: Uuid,
        input: &CreateInventoryCount,
    ) -> Result<Value, DomainError> {
        validate_create(input)?;
        validate_currency(&input.currency)?;
        let scope = authorize_creation(&self.store, actor, input).await?;
        let mut tx = self.store.pool().begin().await?;
        let plan = plan(&mut tx, &scope, input).await?;
        tx.rollback().await?;
        Ok(plan.snapshot)
    }

    /// Create a count only if its locked state matches a previously approved server snapshot.
    /// The approval boundary must supply the immutable snapshot; it is not a client override.
    pub async fn create_guarded(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        key: &str,
        input: &CreateInventoryCount,
        approved: &Value,
    ) -> Result<CommandResult, DomainError> {
        self.create_inner(actor, trace_id, key, input, Some(approved))
            .await
    }
}

pub(super) async fn authorize_creation(
    store: &PgStore,
    actor: Uuid,
    input: &CreateInventoryCount,
) -> Result<crate::model::AuthorizationSnapshot, DomainError> {
    let before = store
        .authorization_revision()
        .await
        .map_err(|_| DomainError::NotFoundOrForbidden)?;
    let scope = authorize(
        store,
        actor,
        "inventory_opening:create",
        Some(input.legal_entity_id),
        Some(input.warehouse_id),
        None,
        None,
        None,
    )
    .await?;
    if before != scope.scope_version {
        return Err(DomainError::NotFoundOrForbidden);
    }
    Ok(scope)
}
