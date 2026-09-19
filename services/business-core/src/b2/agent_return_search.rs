use super::api::B2ApiError;
use crate::{api::AppState, security::RequestContext};
use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use business_query_contracts::{SearchStockDocumentsInput, ValidateInput};
use serde_json::{json, Value};
use sqlx::AssertSqlSafe;
use std::sync::Arc;
use uuid::Uuid;

pub(super) async fn search(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(kind): Path<String>,
    Query(mut input): Query<SearchStockDocumentsInput>,
) -> Result<Json<Value>, B2ApiError> {
    input
        .validate_and_normalize(chrono::Utc::now().date_naive())
        .map_err(|_| {
            B2ApiError::domain(
                super::DomainError::Invalid("invalid return filter".into()),
                c.trace_id,
            )
        })?;
    let (
        table,
        lines,
        orders,
        foreign,
        source,
        party,
        party_key,
        workflow,
        amount,
        cost,
        source_table,
        source_line_fk,
        source_order_line_fk,
        permission,
    ) = match kind.as_str() {
        "sales_return" => (
            "sales_returns",
            "sales_return_lines",
            "sales_orders",
            "sales_order_id",
            "shipment_id",
            "customer_id",
            "customerId",
            "inspection_status",
            "sales_amount",
            "cost_amount",
            "shipment_lines",
            "shipment_line_id",
            "sales_order_line_id",
            "sales_order:read",
        ),
        "purchase_return" => (
            "purchase_returns",
            "purchase_return_lines",
            "purchase_orders",
            "purchase_order_id",
            "goods_receipt_id",
            "supplier_id",
            "supplierId",
            "logistics_status",
            "gross_amount",
            "inventory_cost_amount",
            "goods_receipt_lines",
            "goods_receipt_line_id",
            "purchase_order_line_id",
            "goods_receipt:read",
        ),
        _ => {
            return Err(B2ApiError::domain(
                super::DomainError::NotFoundOrForbidden,
                c.trace_id,
            ))
        }
    };
    let auth = state
        .store
        .snapshot(c.actor_user_id)
        .await
        .map_err(|_| B2ApiError::domain(super::DomainError::NotFoundOrForbidden, c.trace_id))?;
    if !auth.permission_keys.contains(permission) {
        return Err(B2ApiError::domain(
            super::DomainError::NotFoundOrForbidden,
            c.trace_id,
        ));
    }
    let scope = auth.scopes;
    let parties = if kind == "sales_return" {
        scope.customer_ids
    } else {
        scope.supplier_ids
    };
    let order_lines = if kind == "sales_return" {
        "sales_order_lines"
    } else {
        "purchase_order_lines"
    };
    let return_fk = if kind == "sales_return" {
        "sales_return_id"
    } else {
        "purchase_return_id"
    };
    let line_sql=format!("SELECT l.*,ol.brand_id,p.brand_id current_brand_id,sku.code sku_code,sku.name sku_name FROM {lines} l JOIN {source_table} sl ON sl.id=l.{source_line_fk} JOIN {order_lines} ol ON ol.id=sl.{source_order_line_fk} JOIN business_skus sku ON sku.id=l.sku_id JOIN business_products p ON p.id=sku.product_id WHERE l.{return_fk}=r.id");
    // All identifiers are fixed by the family match; all caller values are bound.
    let sql=format!("SELECT jsonb_build_object('id',r.id,'number',r.return_number,'sourceId',r.{source},'orderId',r.{foreign},'legalEntityId',r.legal_entity_id,'businessUnitId',o.business_unit_id,'warehouseId',r.warehouse_id,'brandId',o.brand_id,'{party_key}',r.{party},'businessDate',r.return_date,'currency',r.currency::text,'status',r.status,'workflowStatus',r.{workflow},'version',r.version,'reasonCode',r.reason_code,'businessNote',r.business_note,'amount',r.{amount}::text,'cost',r.{cost}::text,'lines',(SELECT jsonb_agg(jsonb_build_object('returnLineId',x.id,'sourceLineId',x.{source_line_fk},'skuId',x.sku_id,'skuCode',x.sku_code,'skuName',x.sku_name,'quantity',x.quantity::text,'unitCost',x.unit_cost::text,'totalCost',x.total_cost::text,'brandId',x.brand_id,'currentBrandId',x.current_brand_id,'warehouseId',r.warehouse_id) ORDER BY x.id) FROM ({line_sql}) x)) FROM {table} r JOIN {orders} o ON o.id=r.{foreign} WHERE r.legal_entity_id=ANY($1) AND r.{party}=ANY($2) AND o.business_unit_id=ANY($3) AND r.warehouse_id=ANY($4) AND (o.brand_id IS NULL OR o.brand_id=ANY($5)) AND NOT EXISTS(SELECT 1 FROM ({line_sql}) x WHERE (x.brand_id IS NOT NULL AND NOT(x.brand_id=ANY($5))) OR (x.current_brand_id IS NOT NULL AND NOT(x.current_brand_id=ANY($5)))) AND ($6::uuid IS NULL OR r.id=$6) AND ($7::text IS NULL OR strpos(lower(r.return_number),lower($7))>0) AND ($8::uuid IS NULL OR r.{party}=$8) AND ($9::text IS NULL OR r.status=$9) ORDER BY r.return_date DESC,r.id LIMIT $10 OFFSET $11");
    let items: Vec<Value> = sqlx::query_scalar(AssertSqlSafe(sql))
        .bind(scope.legal_entity_ids.into_iter().collect::<Vec<Uuid>>())
        .bind(parties.into_iter().collect::<Vec<_>>())
        .bind(scope.business_unit_ids.into_iter().collect::<Vec<_>>())
        .bind(scope.warehouse_ids.into_iter().collect::<Vec<_>>())
        .bind(scope.brand_ids.into_iter().collect::<Vec<_>>())
        .bind(input.document_id)
        .bind(input.query)
        .bind(input.party_id)
        .bind(input.status)
        .bind(i64::from(input.limit) + 1)
        .bind(i64::from(input.offset))
        .fetch_all(state.store.pool())
        .await
        .map_err(|e| B2ApiError::domain(e.into(), c.trace_id))?;
    Ok(Json(json!({"items":items,"traceId":c.trace_id})))
}

pub(super) async fn sales_detail(
    state: State<Arc<AppState>>,
    context: Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, B2ApiError> {
    detail(state, context, "sales_return", id).await
}

pub(super) async fn purchase_detail(
    state: State<Arc<AppState>>,
    context: Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, B2ApiError> {
    detail(state, context, "purchase_return", id).await
}

pub(super) async fn detail(
    state: State<Arc<AppState>>,
    context: Extension<RequestContext>,
    kind: &str,
    id: Uuid,
) -> Result<Json<Value>, B2ApiError> {
    let trace = context.trace_id;
    let Json(result) = search(
        state,
        context,
        Path(kind.into()),
        Query(SearchStockDocumentsInput {
            document_id: Some(id),
            query: None,
            party_id: None,
            status: None,
            offset: 0,
            limit: 1,
        }),
    )
    .await?;
    let mut item = result["items"]
        .as_array()
        .and_then(|items| items.first())
        .cloned()
        .ok_or_else(|| B2ApiError::domain(super::DomainError::NotFoundOrForbidden, trace))?;
    item["traceId"] = json!(trace);
    Ok(Json(item))
}
