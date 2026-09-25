//! Current, fully authorized adjustment details with version-bound line pagination.
use super::*;
use std::collections::BTreeSet;

/// Pagination for a single adjustment. Later pages require the first page's version.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdjustmentDetailQuery {
    /// Zero-based line offset.
    #[serde(default)]
    pub offset: usize,
    /// Maximum lines per response (1–100).
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Version to bind subsequent pages to the same draft.
    pub expected_version: Option<i64>,
}
fn default_limit() -> usize {
    20
}
impl AdjustmentService {
    /// Read a batch only if every line, referenced order and posted fact is in current scope.
    pub async fn detail(
        &self,
        actor: Uuid,
        id: Uuid,
        query: &AdjustmentDetailQuery,
    ) -> Result<Value, DomainError> {
        if query.limit == 0
            || query.limit > 100
            || query.offset > 10000
            || (query.offset > 0 && query.expected_version.is_none())
            || query.expected_version.is_some_and(|v| v < 1)
        {
            return Err(DomainError::Invalid(
                "invalid adjustment detail pagination".into(),
            ));
        }
        let mut tx = self.store.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;
        let authorization = crate::master_write_authority::snapshot(
            &mut tx,
            actor,
            "profit_adjustment:read",
            false,
        )
        .await?;
        let result = self.detail_on(&mut tx, id, query, &authorization).await?;
        tx.commit().await?;
        Ok(result)
    }
    pub(super) async fn detail_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        id: Uuid,
        query: &AdjustmentDetailQuery,
        authorization: &AuthorizationSnapshot,
    ) -> Result<Value, DomainError> {
        let batch = sqlx::query(
            "SELECT b.*,to_jsonb(b) record FROM operational_adjustment_batches b WHERE id=$1",
        )
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
        let version: i64 = batch.get("version");
        if !authorization
            .scopes
            .legal_entity_ids
            .contains(&batch.get::<Uuid, _>("legal_entity_id"))
        {
            return Err(DomainError::NotFoundOrForbidden);
        }
        if query.expected_version.is_some_and(|v| v != version) {
            return Err(DomainError::VersionConflict);
        }
        let lines=sqlx::query("SELECT l.*,to_jsonb(l)||jsonb_build_object('amount',l.amount::text) record FROM operational_adjustment_lines l WHERE batch_id=$1 ORDER BY line_number,id").bind(id).fetch_all(&mut **tx).await?;
        let mut order_ids = BTreeSet::new();
        let mut business_unit_ids = BTreeSet::new();
        let mut total = Decimal::ZERO;
        let scopes = &authorization.scopes;
        for line in &lines {
            for (column, allowed) in [
                ("customer_id", &scopes.customer_ids),
                ("brand_id", &scopes.brand_ids),
                ("business_unit_id", &scopes.business_unit_ids),
                ("warehouse_id", &scopes.warehouse_ids),
            ] {
                if line
                    .get::<Option<Uuid>, _>(column)
                    .is_some_and(|id| !allowed.contains(&id))
                {
                    return Err(DomainError::NotFoundOrForbidden);
                }
            }
            if let Some(id) = line.get::<Option<Uuid>, _>("business_unit_id") {
                business_unit_ids.insert(id);
            }
            if let Some(id) = line.get::<Option<Uuid>, _>("direct_sales_order_id") {
                order_ids.insert(id);
            }
            let scope: Value = line.get("allocation_scope");
            for key in ["salesOrderIds", "fixedWeights"] {
                for value in scope[key].as_array().into_iter().flatten() {
                    let value = if key == "fixedWeights" {
                        &value["salesOrderId"]
                    } else {
                        value
                    };
                    let id = value
                        .as_str()
                        .and_then(|s| s.parse::<Uuid>().ok())
                        .ok_or(DomainError::NotFoundOrForbidden)?;
                    order_ids.insert(id);
                }
            }
            // Draft allocations are not frozen. Check every currently selected
            // aggregate target before disclosing even a page of the document.
            if !matches!(
                batch.get::<String, _>("status").as_str(),
                "posted" | "reversed"
            ) && !matches!(
                line.get::<String, _>("allocation_basis").as_str(),
                "direct" | "fixed_weight"
            ) {
                order_ids.extend(
                    targets(tx, &batch, line, self.max_targets)
                        .await?
                        .into_iter()
                        .map(|(id, _)| id),
                );
            }
            total = total
                .checked_add(line.get::<Decimal, _>("amount"))
                .ok_or_else(|| DomainError::Invalid("adjustment total overflow".into()))?;
        }
        let allocated:Vec<Uuid>=sqlx::query_scalar("SELECT DISTINCT sales_order_id FROM operational_adjustment_allocations WHERE batch_id=$1").bind(id).fetch_all(&mut **tx).await?;
        order_ids.extend(allocated);
        for order in &order_ids {
            visible_order(tx, *order, authorization).await?;
        }
        let target_business_units: Vec<Uuid> = sqlx::query_scalar(
            "SELECT DISTINCT business_unit_id FROM sales_orders WHERE id=ANY($1) ORDER BY business_unit_id",
        )
        .bind(order_ids.iter().copied().collect::<Vec<_>>())
        .fetch_all(&mut **tx)
        .await?;
        business_unit_ids.extend(target_business_units);
        // Order ownership can change after posting. Historical fact dimensions
        // must also remain accessible, rather than relying only on today's order.
        let outside:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM profit_facts WHERE source_type='operational_adjustment' AND source_id=$1 AND (NOT(legal_entity_id=ANY($2)) OR (customer_id IS NOT NULL AND NOT(customer_id=ANY($3))) OR (brand_id IS NOT NULL AND NOT(brand_id=ANY($4))) OR (business_unit_id IS NOT NULL AND NOT(business_unit_id=ANY($5))) OR (warehouse_id IS NOT NULL AND NOT(warehouse_id=ANY($6)))))").bind(id).bind(scopes.legal_entity_ids.iter().copied().collect::<Vec<_>>()).bind(scopes.customer_ids.iter().copied().collect::<Vec<_>>()).bind(scopes.brand_ids.iter().copied().collect::<Vec<_>>()).bind(scopes.business_unit_ids.iter().copied().collect::<Vec<_>>()).bind(scopes.warehouse_ids.iter().copied().collect::<Vec<_>>()).fetch_one(&mut **tx).await?;
        if outside {
            return Err(DomainError::NotFoundOrForbidden);
        }
        let has_unattributed_brand_targets:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sales_orders WHERE id=ANY($1) AND brand_id IS NULL) OR EXISTS(SELECT 1 FROM profit_facts WHERE sales_order_id=ANY($1) AND brand_id IS NULL)").bind(order_ids.iter().copied().collect::<Vec<_>>()).fetch_one(&mut **tx).await?;
        let total_lines = lines.len();
        let items: Vec<Value> = lines
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .map(|row| row.get("record"))
            .collect();
        let next = query.offset + items.len();
        let result = json!({"schemaVersion":1,"batch":batch.get::<Value,_>("record"),"businessUnitIds":business_unit_ids,"lines":items,"totalAmount":total.to_string(),"targetOrderCount":order_ids.len(),"hasUnattributedBrandTargets":has_unattributed_brand_targets,"scope":authorization.scopes,"pagination":{"offset":query.offset,"limit":query.limit,"total":total_lines,"nextOffset":if next<total_lines {Some(next)}else{None}},"version":version,"boundary":"management_only_not_general_ledger"});
        Ok(result)
    }
}
pub(super) async fn visible_order(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    authorization: &AuthorizationSnapshot,
) -> Result<(), DomainError> {
    let s = &authorization.scopes;
    let visible:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sales_orders o WHERE o.id=$1 AND o.legal_entity_id=ANY($2) AND o.customer_id=ANY($3) AND (o.brand_id IS NULL OR o.brand_id=ANY($4)) AND o.business_unit_id=ANY($5) AND NOT EXISTS(SELECT 1 FROM sales_order_lines l WHERE l.sales_order_id=o.id AND NOT(l.warehouse_id=ANY($6))) AND NOT EXISTS(SELECT 1 FROM profit_facts f WHERE f.sales_order_id=o.id AND (NOT(f.legal_entity_id=ANY($2)) OR (f.customer_id IS NOT NULL AND NOT(f.customer_id=ANY($3))) OR (f.brand_id IS NOT NULL AND NOT(f.brand_id=ANY($4))) OR (f.business_unit_id IS NOT NULL AND NOT(f.business_unit_id=ANY($5))) OR (f.warehouse_id IS NOT NULL AND NOT(f.warehouse_id=ANY($6))))))")
        .bind(id).bind(s.legal_entity_ids.iter().copied().collect::<Vec<_>>()).bind(s.customer_ids.iter().copied().collect::<Vec<_>>()).bind(s.brand_ids.iter().copied().collect::<Vec<_>>()).bind(s.business_unit_ids.iter().copied().collect::<Vec<_>>()).bind(s.warehouse_ids.iter().copied().collect::<Vec<_>>()).fetch_one(&mut **tx).await?;
    if visible {
        Ok(())
    } else {
        Err(DomainError::NotFoundOrForbidden)
    }
}
