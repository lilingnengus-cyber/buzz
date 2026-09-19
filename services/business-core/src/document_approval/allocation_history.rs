use super::*;
use axum::extract::Query;
use business_query_contracts::{
    SearchFinancialDocumentsInput, SettlementAllocationsInput, ValidateInput,
};

pub(super) async fn search(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(kind): Path<String>,
    Query(mut input): Query<SettlementAllocationsInput>,
) -> Response {
    if input
        .validate_and_normalize(chrono::Utc::now().date_naive())
        .is_err()
    {
        return approval_error(StatusCode::BAD_REQUEST, "invalid_filter", c.trace_id);
    }
    match history(&state, c.actor_user_id, &kind, &input).await {
        Ok(mut value) => {
            value["traceId"] = json!(c.trace_id);
            Json(value).into_response()
        }
        Err(e) => store_error(e, c.trace_id),
    }
}
async fn history(
    state: &AppState,
    actor: Uuid,
    kind: &str,
    input: &SettlementAllocationsInput,
) -> Result<serde_json::Value, StoreError> {
    let source = settlement::value(state, actor, kind, input.source_document_id).await?;
    let (sql,target_kind)=match kind {
        "customer_receipt" => ("SELECT a.id,a.receivable_id target_id,a.amount::text,EXISTS(SELECT 1 FROM receivable_allocations r WHERE r.reverses_allocation_id=a.id) reversed FROM receivable_allocations a WHERE a.receipt_id=$1 AND a.allocation_type='apply' ORDER BY a.created_at DESC,a.id LIMIT $2 OFFSET $3","receivable"),
        "supplier_payment" => ("SELECT a.id,a.payable_id target_id,a.amount::text,EXISTS(SELECT 1 FROM payable_allocations r WHERE r.reverses_allocation_id=a.id) reversed FROM payable_allocations a WHERE a.supplier_payment_id=$1 AND a.allocation_type='apply' ORDER BY a.created_at DESC,a.id LIMIT $2 OFFSET $3","payable"),
        _=>return Err(StoreError::NotFoundOrForbidden),
    };
    let rows = sqlx::query(sql)
        .bind(input.source_document_id)
        .bind(i64::from(input.limit) + 1)
        .bind(i64::from(input.offset))
        .fetch_all(state.store.pool())
        .await?;
    let has_more = rows.len() > input.limit as usize;
    let mut items = Vec::new();
    for row in rows.into_iter().take(input.limit as usize) {
        let filter = SearchFinancialDocumentsInput {
            document_id: Some(row.get("target_id")),
            query: None,
            party_id: None,
            status: None,
            offset: 0,
            limit: 1,
        };
        let target = financial_documents::rows(state, actor, target_kind, &filter)
            .await?
            .into_iter()
            .next();
        if let Some(target) = target {
            items.push(json!({"id":row.get::<Uuid,_>("id"),"sourceDocumentId":input.source_document_id,"source":source,"target":target,"amount":row.get::<String,_>("amount"),"reversed":row.get::<bool,_>("reversed")}));
        }
    }
    Ok(
        json!({"items":items,"hasMore":has_more,"nextOffset":has_more.then_some(input.offset+input.limit)}),
    )
}
