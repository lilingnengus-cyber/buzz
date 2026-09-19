//! Service-authenticated return source lookup and version-bound draft creation.
use super::{
    api::{key, B2ApiError},
    returns::CreateReturn,
    DomainError,
};
use crate::{api::AppState, security::RequestContext};
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post},
    Extension, Json, Router,
};
use serde_json::{json, Value};
use sqlx::{AssertSqlSafe, Row};
use std::sync::Arc;
use uuid::Uuid;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/v1/agent-return-documents/{kind}",
            get(super::agent_return_search::search),
        )
        .route("/v1/agent-return-sources/{kind}/{id}", get(source))
        .route("/v1/agent-drafts/returns/{kind}", post(create))
}
fn sales(kind: &str) -> Result<bool, DomainError> {
    match kind {
        "sales_return" => Ok(true),
        "purchase_return" => Ok(false),
        _ => Err(DomainError::NotFoundOrForbidden),
    }
}
async fn create(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(kind): Path<String>,
    headers: HeaderMap,
    Json(input): Json<CreateReturn>,
) -> Result<Json<Value>, B2ApiError> {
    let sales = sales(&kind).map_err(|e| B2ApiError::domain(e, c.trace_id))?;
    if input
        .expected_source_version
        .is_none_or(|version| version < 1)
    {
        return Err(B2ApiError::domain(
            DomainError::Invalid("expectedSourceVersion is required and must be positive".into()),
            c.trace_id,
        ));
    }
    let key = key(&headers, c.trace_id)?;
    let result = if sales {
        state
            .returns
            .create_sales_return(c.actor_user_id, c.trace_id, key, &input)
            .await
    } else {
        state
            .returns
            .create_purchase_return(c.actor_user_id, c.trace_id, key, &input)
            .await
    }
    .map_err(|e| B2ApiError::domain(e, c.trace_id))?;
    Ok(Json(
        json!({"id":result.id,"number":result.number,"status":result.status,"version":result.version,"idempotentReplay":result.idempotent_replay,"traceId":c.trace_id}),
    ))
}
async fn source(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path((kind, id)): Path<(String, Uuid)>,
) -> Result<Json<Value>, B2ApiError> {
    source_value(&state, c.actor_user_id, &kind, id)
        .await
        .map(|item| Json(json!({"item":item,"traceId":c.trace_id})))
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
async fn source_value(
    state: &AppState,
    actor: Uuid,
    kind: &str,
    id: Uuid,
) -> Result<Value, DomainError> {
    let sales = sales(kind)?;
    let (header_sql, line_sql, permission) = if sales {
        ("SELECT s.id,s.shipment_number number,s.legal_entity_id,s.warehouse_id,s.customer_id party_id,s.shipment_date business_date,s.currency::text currency,s.status,s.version,o.business_unit_id,o.brand_id FROM shipments s JOIN sales_orders o ON o.id=s.sales_order_id WHERE s.id=$1",
        "SELECT l.id,l.sku_id,sku.code sku_code,sku.name sku_name,l.quantity quantity,l.sales_amount amount,l.unit_cost unit_cost,l.total_cost total_cost,ol.brand_id,p.brand_id current_brand_id,COALESCE((SELECT sum(rl.quantity) FROM sales_return_lines rl JOIN sales_returns r ON r.id=rl.sales_return_id WHERE rl.shipment_line_id=l.id AND r.status IN ('draft','confirmed')),0) allocated FROM shipment_lines l JOIN sales_order_lines ol ON ol.id=l.sales_order_line_id JOIN business_skus sku ON sku.id=l.sku_id JOIN business_products p ON p.id=sku.product_id WHERE l.shipment_id=$1 ORDER BY l.id",
        "sales_order:read")
    } else {
        ("SELECT s.id,s.goods_receipt_number number,s.legal_entity_id,s.warehouse_id,s.supplier_id party_id,s.receipt_date business_date,s.currency::text currency,s.status,s.version,o.business_unit_id,o.brand_id FROM goods_receipts s JOIN purchase_orders o ON o.id=s.purchase_order_id WHERE s.id=$1",
        "SELECT l.id,l.sku_id,sku.code sku_code,sku.name sku_name,l.received_quantity quantity,l.gross_amount amount,l.provisional_unit_cost unit_cost,l.provisional_total_cost total_cost,ol.brand_id,p.brand_id current_brand_id,COALESCE((SELECT sum(rl.quantity) FROM purchase_return_lines rl JOIN purchase_returns r ON r.id=rl.purchase_return_id WHERE rl.goods_receipt_line_id=l.id AND r.status IN ('draft','confirmed')),0) allocated FROM goods_receipt_lines l JOIN purchase_order_lines ol ON ol.id=l.purchase_order_line_id JOIN business_skus sku ON sku.id=l.sku_id JOIN business_products p ON p.id=sku.product_id WHERE l.goods_receipt_id=$1 ORDER BY l.id",
        "goods_receipt:read")
    };
    let row = sqlx::query(header_sql)
        .bind(id)
        .fetch_optional(state.store.pool())
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
    let auth = state
        .store
        .snapshot(actor)
        .await
        .map_err(|_| DomainError::NotFoundOrForbidden)?;
    let parties = if sales {
        &auth.scopes.customer_ids
    } else {
        &auth.scopes.supplier_ids
    };
    if !auth.permission_keys.contains(permission)
        || !auth
            .scopes
            .legal_entity_ids
            .contains(&row.get("legal_entity_id"))
        || !auth.scopes.warehouse_ids.contains(&row.get("warehouse_id"))
        || !parties.contains(&row.get("party_id"))
    {
        return Err(DomainError::NotFoundOrForbidden);
    }
    super::return_scope::check_source(&state.store, actor, sales, id).await?;
    // The only interpolated text is one of the two literal SQL statements above.
    let lines:Vec<Value>=sqlx::query_scalar(AssertSqlSafe(format!("SELECT jsonb_build_object('sourceLineId',x.id,'skuId',x.sku_id,'skuCode',x.sku_code,'skuName',x.sku_name,'brandId',x.brand_id,'currentBrandId',x.current_brand_id,'sourceQuantity',x.quantity::text,'allocatedReturnQuantity',x.allocated::text,'returnableQuantity',GREATEST(x.quantity-x.allocated,0)::text,'sourceAmount',x.amount::text,'sourceUnitCost',x.unit_cost::text,'sourceTotalCost',x.total_cost::text) FROM ({line_sql}) x"))).bind(id).fetch_all(state.store.pool()).await?;
    Ok(
        json!({"id":id,"number":row.get::<String,_>("number"),"version":row.get::<i64,_>("version"),"status":row.get::<String,_>("status"),"legalEntityId":row.get::<Uuid,_>("legal_entity_id"),"warehouseId":row.get::<Uuid,_>("warehouse_id"),"businessUnitId":row.get::<Uuid,_>("business_unit_id"),"brandId":row.get::<Option<Uuid>,_>("brand_id"),"customerId":if sales {Some(row.get::<Uuid,_>("party_id"))}else{None},"supplierId":if sales {None}else{Some(row.get::<Uuid,_>("party_id"))},"businessDate":row.get::<chrono::NaiveDate,_>("business_date"),"currency":row.get::<String,_>("currency"),"lines":lines,"canCreateDraft":row.get::<String,_>("status")=="confirmed" && auth.permission_keys.contains(if sales {"shipment:reverse"}else{"goods_receipt:reverse"})}),
    )
}
