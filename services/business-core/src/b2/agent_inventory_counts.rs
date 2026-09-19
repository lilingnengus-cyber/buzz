//! Scoped count lookup for agents; authorization is applied before pagination.
use super::{api::B2ApiError, common::authorize, DomainError};
use crate::{api::AppState, security::RequestContext};
use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use business_query_contracts::{
    SearchInventoryCountOptionsInput, SearchInventoryCountsInput, ValidateInput,
};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

pub(super) async fn search(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Query(input): Query<SearchInventoryCountsInput>,
) -> Result<Json<Value>, B2ApiError> {
    query(state, c, input, false).await
}

async fn query(
    state: Arc<AppState>,
    c: RequestContext,
    mut input: SearchInventoryCountsInput,
    include_lines: bool,
) -> Result<Json<Value>, B2ApiError> {
    input
        .validate_and_normalize(chrono::Utc::now().date_naive())
        .map_err(|_| {
            B2ApiError::domain(
                DomainError::Invalid("invalid inventory count filter".into()),
                c.trace_id,
            )
        })?;
    let authority = authorize(
        &state.store,
        c.actor_user_id,
        "inventory:read",
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .map_err(|e| B2ApiError::domain(e, c.trace_id))?;
    let scope = authority.scopes;
    let items:Vec<Value> = sqlx::query_scalar(r#"
SELECT jsonb_build_object(
 'id',t.id,'number',t.count_number,'legalEntityId',t.legal_entity_id,
 'warehouseId',t.warehouse_id,'warehouseCode',w.code,'warehouseName',w.name,
 'businessUnitId',w.business_unit_id,'snapshotBusinessUnitId',t.snapshot_business_unit_id,
 'scopeSnapshotCaptured',t.scope_snapshot_captured,'businessDate',t.count_date,
 'currency',t.currency::text,'status',t.status,'version',t.version,
 'retainsFreeze',t.status IN ('counting','counted'),'updatedAt',t.updated_at,
 'lineCount',(SELECT count(*) FROM inventory_count_lines l WHERE l.inventory_count_id=t.id),
 'varianceLineCount',(SELECT count(*) FROM inventory_count_lines l WHERE l.inventory_count_id=t.id AND COALESCE(l.variance_quantity,0)<>0),
 'varianceValue',(SELECT COALESCE(sum(l.variance_value),0)::text FROM inventory_count_lines l WHERE l.inventory_count_id=t.id),
 'lines',CASE WHEN $13::boolean THEN (SELECT jsonb_agg(jsonb_build_object(
  'id',l.id,'skuId',l.sku_id,'skuCode',sku.code,'skuName',sku.name,
  'brandId',p.brand_id,'snapshotBrandId',l.snapshot_brand_id,
  'snapshotOnHandQuantity',l.snapshot_on_hand_quantity::text,
  'snapshotReservedQuantity',l.snapshot_reserved_quantity::text,
  'snapshotQuarantinedQuantity',l.snapshot_quarantined_quantity::text,
  'snapshotAverageUnitCost',l.snapshot_average_unit_cost::text,
  'actualOnHandQuantity',l.actual_on_hand_quantity::text,
  'surplusUnitCost',l.surplus_unit_cost::text,
  'varianceQuantity',l.variance_quantity::text,'varianceValue',l.variance_value::text
 ) ORDER BY sku.code,l.id) FROM inventory_count_lines l
 JOIN business_skus sku ON sku.id=l.sku_id JOIN business_products p ON p.id=sku.product_id
 WHERE l.inventory_count_id=t.id)
 ELSE (SELECT jsonb_agg(brands) FROM (SELECT DISTINCT jsonb_build_object('brandId',p.brand_id,'snapshotBrandId',l.snapshot_brand_id) AS brands
 FROM inventory_count_lines l JOIN business_skus sku ON sku.id=l.sku_id JOIN business_products p ON p.id=sku.product_id
 WHERE l.inventory_count_id=t.id) scoped) END)
FROM inventory_count_tasks t JOIN business_warehouses w ON w.id=t.warehouse_id
WHERE t.legal_entity_id=ANY($1) AND t.warehouse_id=ANY($2) AND w.business_unit_id=ANY($3)
 AND (t.snapshot_business_unit_id IS NULL OR t.snapshot_business_unit_id=ANY($3))
 AND NOT EXISTS(SELECT 1 FROM inventory_count_lines l JOIN business_skus sku ON sku.id=l.sku_id
 JOIN business_products p ON p.id=sku.product_id WHERE l.inventory_count_id=t.id
 AND ((p.brand_id IS NOT NULL AND NOT(p.brand_id=ANY($4)))
 OR (l.snapshot_brand_id IS NOT NULL AND NOT(l.snapshot_brand_id=ANY($4)))))
 AND ($5::uuid IS NULL OR t.id=$5)
 AND ($6::text IS NULL OR strpos(lower(t.count_number),lower($6))>0)
 AND ($7::uuid IS NULL OR t.legal_entity_id=$7)
 AND ($8::uuid IS NULL OR t.warehouse_id=$8)
 AND ($9::uuid IS NULL OR EXISTS(SELECT 1 FROM inventory_count_lines l WHERE l.inventory_count_id=t.id AND l.sku_id=$9))
 AND ($10::text IS NULL OR t.status=$10)
ORDER BY t.count_date DESC,t.count_number DESC,t.id LIMIT $11 OFFSET $12
"#)
    .bind(scope.legal_entity_ids.into_iter().collect::<Vec<_>>())
    .bind(scope.warehouse_ids.into_iter().collect::<Vec<_>>())
    .bind(scope.business_unit_ids.into_iter().collect::<Vec<_>>())
    .bind(scope.brand_ids.into_iter().collect::<Vec<_>>())
    .bind(input.document_id).bind(input.query).bind(input.legal_entity_id).bind(input.warehouse_id)
    .bind(input.sku_id).bind(input.status).bind(i64::from(input.limit)+1).bind(i64::from(input.offset)).bind(include_lines)
    .fetch_all(state.store.pool()).await.map_err(|e| B2ApiError::domain(e.into(),c.trace_id))?;
    Ok(Json(json!({"items":items,"traceId":c.trace_id})))
}

pub(super) async fn detail(
    state: State<Arc<AppState>>,
    context: Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, B2ApiError> {
    let trace = context.trace_id;
    let Json(result) = query(
        state.0,
        context.0,
        SearchInventoryCountsInput {
            document_id: Some(id),
            query: None,
            legal_entity_id: None,
            warehouse_id: None,
            sku_id: None,
            status: None,
            offset: 0,
            limit: 1,
        },
        true,
    )
    .await?;
    let item = result["items"]
        .as_array()
        .and_then(|items| items.first())
        .cloned()
        .ok_or_else(|| B2ApiError::domain(DomainError::NotFoundOrForbidden, trace))?;
    Ok(Json(json!({"item":item,"traceId":trace})))
}

pub(super) async fn options(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Query(mut input): Query<SearchInventoryCountOptionsInput>,
) -> Result<Json<Value>, B2ApiError> {
    input
        .validate_and_normalize(chrono::Utc::now().date_naive())
        .map_err(|_| {
            B2ApiError::domain(
                DomainError::Invalid("invalid inventory count option filter".into()),
                c.trace_id,
            )
        })?;
    let authority = authorize(
        &state.store,
        c.actor_user_id,
        "inventory:read",
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .map_err(|e| B2ApiError::domain(e, c.trace_id))?;
    let scope = authority.scopes;
    let items:Vec<Value>=sqlx::query_scalar(r#"
SELECT jsonb_build_object('legalEntityId',b.legal_entity_id,'currency',e.functional_currency::text,
 'warehouseId',b.warehouse_id,'warehouseCode',w.code,'warehouseName',w.name,'businessUnitId',w.business_unit_id,
 'skuId',b.sku_id,'skuCode',s.code,'skuName',s.name,'brandId',p.brand_id,
 'onHandQuantity',b.on_hand_quantity::text,'reservedQuantity',b.reserved_quantity::text,
 'quarantinedQuantity',b.quarantined_quantity::text,'inventoryValue',b.inventory_value::text,
 'averageUnitCost',b.average_unit_cost::text,'version',b.version)
FROM inventory_balances b JOIN business_legal_entities e ON e.id=b.legal_entity_id
 JOIN business_warehouses w ON w.id=b.warehouse_id JOIN business_skus s ON s.id=b.sku_id
 JOIN business_products p ON p.id=s.product_id
WHERE b.legal_entity_id=ANY($1) AND b.warehouse_id=ANY($2) AND w.business_unit_id=ANY($3)
 AND (p.brand_id IS NULL OR p.brand_id=ANY($4))
 AND NOT EXISTS(SELECT 1 FROM inventory_count_tasks t JOIN inventory_count_lines l ON l.inventory_count_id=t.id
 WHERE t.status IN ('counting','counted') AND t.legal_entity_id=b.legal_entity_id AND t.warehouse_id=b.warehouse_id AND l.sku_id=b.sku_id)
 AND ($5::text IS NULL OR strpos(lower(s.code),lower($5))>0 OR strpos(lower(s.name),lower($5))>0)
 AND ($6::uuid IS NULL OR b.legal_entity_id=$6) AND ($7::uuid IS NULL OR b.warehouse_id=$7)
 AND ($8::uuid IS NULL OR b.sku_id=$8)
ORDER BY w.code,s.code,b.legal_entity_id,b.warehouse_id,b.sku_id LIMIT $9 OFFSET $10
"#)
    .bind(scope.legal_entity_ids.into_iter().collect::<Vec<_>>())
    .bind(scope.warehouse_ids.into_iter().collect::<Vec<_>>())
    .bind(scope.business_unit_ids.into_iter().collect::<Vec<_>>())
    .bind(scope.brand_ids.into_iter().collect::<Vec<_>>())
    .bind(input.query).bind(input.legal_entity_id).bind(input.warehouse_id).bind(input.sku_id)
    .bind(i64::from(input.limit)+1).bind(i64::from(input.offset))
    .fetch_all(state.store.pool()).await.map_err(|e|B2ApiError::domain(e.into(),c.trace_id))?;
    Ok(Json(json!({"items":items,"traceId":c.trace_id})))
}
