use super::*;
use axum::extract::Query;
use business_query_contracts::{SearchFinancialDocumentsInput, ValidateInput};
use sqlx::AssertSqlSafe;

pub(super) async fn search(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(kind): Path<String>,
    Query(mut input): Query<SearchFinancialDocumentsInput>,
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

pub(super) async fn rows(
    state: &AppState,
    actor: Uuid,
    kind: &str,
    input: &SearchFinancialDocumentsInput,
) -> Result<Vec<serde_json::Value>, StoreError> {
    // All SQL identifiers/fragments below are selected from this literal catalog. User values are bound.
    let (table,number,party,join,date,amounts,stock) = match kind {
        "customer_receipt" => ("customer_receipts","receipt_number","customer_id","JOIN business_customers o ON o.id=t.customer_id","receipt_date","'amount',t.amount::text,'allocatedAmount',t.allocated_amount::text,'unappliedAmount',t.unapplied_amount::text",false),
        "supplier_payment" => ("supplier_payments","supplier_payment_number","supplier_id","JOIN business_suppliers o ON o.id=t.supplier_id","payment_date","'amount',t.amount::text,'allocatedAmount',t.allocated_amount::text,'unappliedAmount',t.unapplied_amount::text",false),
        "receivable" => ("trade_receivables","receivable_number","customer_id","JOIN sales_orders o ON o.id=t.sales_order_id JOIN shipments s ON s.id=t.shipment_id","due_date","'originalAmount',t.original_amount::text,'settledAmount',t.settled_amount::text,'openAmount',t.open_amount::text",true),
        "payable" => ("trade_payables","payable_number","supplier_id","JOIN purchase_orders o ON o.id=t.purchase_order_id JOIN goods_receipts s ON s.id=t.goods_receipt_id","due_date","'originalAmount',t.original_amount::text,'settledAmount',t.settled_amount::text,'openAmount',t.open_amount::text",true),
        _=> return Err(StoreError::NotFoundOrForbidden),
    };
    let authority = state.store.snapshot(actor).await?;
    if !authority.permission_keys.contains(&format!("{kind}:read")) {
        return Err(StoreError::NotFoundOrForbidden);
    }
    let scope = authority.scopes;
    let party_key = if party == "customer_id" {
        "customerId"
    } else {
        "supplierId"
    };
    let parties = if party == "customer_id" {
        scope.customer_ids
    } else {
        scope.supplier_ids
    };
    let (extra, condition) = if stock {
        let (lines, fk) = if kind == "receivable" {
            ("sales_order_lines", "sales_order_id")
        } else {
            ("purchase_order_lines", "purchase_order_id")
        };
        (format!(",'warehouseId',s.warehouse_id,'brandId',o.brand_id,'lines',(SELECT COALESCE(jsonb_agg(jsonb_build_object('brandId',b.brand_id)), '[]'::jsonb) FROM (SELECT DISTINCT brand_id FROM {lines} WHERE {fk}=o.id AND brand_id IS NOT NULL ORDER BY brand_id) b)"), format!(" AND s.warehouse_id=ANY($10) AND (o.brand_id IS NULL OR o.brand_id=ANY($11)) AND NOT EXISTS(SELECT 1 FROM {lines} l WHERE l.{fk}=o.id AND l.brand_id IS NOT NULL AND NOT(l.brand_id=ANY($11)))"))
    } else {
        (
            String::new(),
            " AND cardinality($10::uuid[])>=0 AND cardinality($11::uuid[])>=0".into(),
        )
    };
    let sql=format!("SELECT jsonb_build_object('id',t.id,'number',t.{number},'legalEntityId',t.legal_entity_id,'businessUnitId',o.business_unit_id,'{party_key}',t.{party},'currency',t.currency::text,'businessDate',t.{date},'status',t.status,'version',t.version,{amounts}{extra}) FROM {table} t {join} WHERE t.legal_entity_id=ANY($1) AND t.{party}=ANY($2) AND o.business_unit_id=ANY($3) AND ($4::uuid IS NULL OR t.id=$4) AND ($5::text IS NULL OR strpos(lower(t.{number}),lower($5))>0) AND ($6::uuid IS NULL OR t.{party}=$6) AND ($7::text IS NULL OR t.status=$7){condition} ORDER BY t.{date} DESC,t.id LIMIT $8 OFFSET $9");
    sqlx::query_scalar(AssertSqlSafe(sql))
        .bind(scope.legal_entity_ids.into_iter().collect::<Vec<_>>())
        .bind(parties.into_iter().collect::<Vec<_>>())
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
