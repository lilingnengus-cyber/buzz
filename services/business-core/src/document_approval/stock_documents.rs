use super::*;
use axum::extract::Query;
use business_query_contracts::{SearchStockDocumentsInput, ValidateInput};
use sqlx::AssertSqlSafe;

pub(super) async fn search(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(kind): Path<String>,
    Query(mut input): Query<SearchStockDocumentsInput>,
) -> Response {
    if input
        .validate_and_normalize(chrono::Utc::now().date_naive())
        .is_err()
    {
        return approval_error(StatusCode::BAD_REQUEST, "invalid_filter", c.trace_id);
    }
    match rows(&state, c.actor_user_id, &kind, &input).await {
        Ok(items) => Json(json!({"items":items,"traceId":c.trace_id})).into_response(),
        Err(e) => store_error(e, c.trace_id),
    }
}
async fn rows(
    state: &AppState,
    actor: Uuid,
    kind: &str,
    input: &SearchStockDocumentsInput,
) -> Result<Vec<serde_json::Value>, StoreError> {
    let (table, number, date, permission) = match kind {
        "shipment" => (
            "shipments",
            "shipment_number",
            "shipment_date",
            "sales_order:read",
        ),
        "goods_receipt" => (
            "goods_receipts",
            "goods_receipt_number",
            "receipt_date",
            "goods_receipt:read",
        ),
        "inventory_opening" => (
            "inventory_opening_batches",
            "batch_number",
            "business_date",
            "inventory:read",
        ),
        _ => return Err(StoreError::NotFoundOrForbidden),
    };
    let auth = state.store.snapshot(actor).await?;
    if !auth.permission_keys.contains(permission) {
        return Err(StoreError::NotFoundOrForbidden);
    }
    let scope = auth.scopes;
    let (join, extra, condition, parties) = if kind == "inventory_opening" {
        (String::new(),"'lines',(SELECT jsonb_agg(jsonb_build_object('warehouseId',l.warehouse_id,'brandId',p.brand_id,'skuId',l.sku_id,'quantity',l.quantity::text,'totalCost',l.total_cost::text) ORDER BY l.line_number) FROM inventory_opening_lines l JOIN business_skus s ON s.id=l.sku_id JOIN business_products p ON p.id=s.product_id WHERE l.batch_id=t.id)".to_string(),
        "AND $6::uuid IS NULL AND cardinality($2::uuid[])>=0 AND cardinality($3::uuid[])>=0 AND EXISTS(SELECT 1 FROM inventory_opening_lines l WHERE l.batch_id=t.id) AND NOT EXISTS(SELECT 1 FROM inventory_opening_lines l JOIN business_skus s ON s.id=l.sku_id JOIN business_products p ON p.id=s.product_id WHERE l.batch_id=t.id AND (NOT(l.warehouse_id=ANY($10)) OR (p.brand_id IS NOT NULL AND NOT(p.brand_id=ANY($11)))))".to_string(),Vec::<Uuid>::new())
    } else {
        let (orders, lines, foreign, party, party_key, parties) = if kind == "shipment" {
            (
                "sales_orders",
                "sales_order_lines",
                "sales_order_id",
                "customer_id",
                "customerId",
                scope.customer_ids.iter().copied().collect::<Vec<_>>(),
            )
        } else {
            (
                "purchase_orders",
                "purchase_order_lines",
                "purchase_order_id",
                "supplier_id",
                "supplierId",
                scope.supplier_ids.iter().copied().collect::<Vec<_>>(),
            )
        };
        (format!("JOIN {orders} o ON o.id=t.{foreign}"),format!("'warehouseId',t.warehouse_id,'businessUnitId',o.business_unit_id,'{party_key}',t.{party},'orderId',o.id,'brandId',o.brand_id,'lines',(SELECT jsonb_agg(jsonb_build_object('warehouseId',l.warehouse_id,'brandId',l.brand_id) ORDER BY l.line_number) FROM {lines} l WHERE l.{foreign}=o.id)"),format!("AND t.{party}=ANY($2) AND o.business_unit_id=ANY($3) AND ($6::uuid IS NULL OR t.{party}=$6) AND t.warehouse_id=ANY($10) AND (o.brand_id IS NULL OR o.brand_id=ANY($11)) AND NOT EXISTS(SELECT 1 FROM {lines} l WHERE l.{foreign}=o.id AND (NOT(l.warehouse_id=ANY($10)) OR (l.brand_id IS NOT NULL AND NOT(l.brand_id=ANY($11)))))"),parties)
    };
    // Identifiers come only from the fixed document catalog; all user values are bound.
    let sql=format!("SELECT jsonb_build_object('id',t.id,'number',t.{number},'legalEntityId',t.legal_entity_id,'businessDate',t.{date},'currency',t.currency::text,'status',t.status,'version',t.version,{extra}) FROM {table} t {join} WHERE t.legal_entity_id=ANY($1) AND ($4::uuid IS NULL OR t.id=$4) AND ($5::text IS NULL OR strpos(lower(t.{number}),lower($5))>0) AND ($7::text IS NULL OR t.status=$7) {condition} ORDER BY t.{date} DESC,t.id LIMIT $8 OFFSET $9");
    sqlx::query_scalar(AssertSqlSafe(sql))
        .bind(scope.legal_entity_ids.into_iter().collect::<Vec<_>>())
        .bind(parties)
        .bind(scope.business_unit_ids.into_iter().collect::<Vec<_>>())
        .bind(input.document_id)
        .bind(&input.query)
        .bind(input.party_id)
        .bind(&input.status)
        .bind(i64::from(input.limit) + 1)
        .bind(i64::from(input.offset))
        .bind(scope.warehouse_ids.into_iter().collect::<Vec<_>>())
        .bind(scope.brand_ids.into_iter().collect::<Vec<_>>())
        .fetch_all(state.store.pool())
        .await
        .map_err(StoreError::from)
}
