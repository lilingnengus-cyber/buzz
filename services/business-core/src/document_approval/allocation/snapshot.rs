use super::*;
use rust_decimal::Decimal;

pub(super) async fn snapshot(
    state: &AppState,
    actor: Uuid,
    kind: &str,
    input: &PrepareAllocation,
) -> Result<Value, StoreError> {
    let (source_kind, permission) = family(kind)?;
    if input.allocations.is_empty()
        || input.allocations.len() > 100
        || input.expected_source_version <= 0
    {
        return Err(StoreError::Invalid(
            "one to 100 allocation targets required".into(),
        ));
    }
    let source = settlement::value(state, actor, source_kind, input.source_document_id).await?;
    if source["version"].as_i64() != Some(input.expected_source_version)
        || !matches!(
            source["status"].as_str(),
            Some("confirmed" | "partially_allocated")
        )
    {
        return Err(StoreError::Conflict);
    }
    let authority = state.store.snapshot(actor).await?;
    let read = if source_kind == "customer_receipt" {
        "receivable:read"
    } else {
        "payable:read"
    };
    if !authority.permission_keys.contains(permission) || !authority.permission_keys.contains(read)
    {
        return Err(StoreError::NotFoundOrForbidden);
    }
    let scope = &authority.scopes;
    let sql = if source_kind == "customer_receipt" {
        "SELECT t.id,t.receivable_number number,t.legal_entity_id,t.customer_id party_id,t.currency::text,t.open_amount,t.status,t.version,o.business_unit_id,o.brand_id,s.warehouse_id,ARRAY(SELECT DISTINCT brand_id FROM sales_order_lines WHERE sales_order_id=t.sales_order_id AND brand_id IS NOT NULL ORDER BY brand_id) line_brands FROM trade_receivables t JOIN sales_orders o ON o.id=t.sales_order_id JOIN shipments s ON s.id=t.shipment_id WHERE t.id=$1"
    } else {
        "SELECT t.id,t.payable_number number,t.legal_entity_id,t.supplier_id party_id,t.currency::text,t.open_amount,t.status,t.version,o.business_unit_id,o.brand_id,s.warehouse_id,ARRAY(SELECT DISTINCT brand_id FROM purchase_order_lines WHERE purchase_order_id=t.purchase_order_id AND brand_id IS NOT NULL ORDER BY brand_id) line_brands FROM trade_payables t JOIN purchase_orders o ON o.id=t.purchase_order_id JOIN goods_receipts s ON s.id=t.goods_receipt_id WHERE t.id=$1"
    };
    let party_key = if source_kind == "customer_receipt" {
        "customerId"
    } else {
        "supplierId"
    };
    let mut total = Decimal::ZERO;
    let mut seen = std::collections::BTreeSet::new();
    let mut targets = Vec::new();
    for allocation in &input.allocations {
        let amount = allocation
            .amount
            .positive("allocation amount")
            .map_err(StoreError::Invalid)?;
        if allocation.expected_version <= 0 || !seen.insert(allocation.document_id) {
            return Err(StoreError::Invalid("duplicate or invalid target".into()));
        }
        let row = sqlx::query(sql)
            .bind(allocation.document_id)
            .fetch_optional(state.store.pool())
            .await?
            .ok_or(StoreError::NotFoundOrForbidden)?;
        let legal: Uuid = row.get("legal_entity_id");
        let party: Uuid = row.get("party_id");
        let unit: Uuid = row.get("business_unit_id");
        let warehouse: Uuid = row.get("warehouse_id");
        let brand: Option<Uuid> = row.get("brand_id");
        let brands: Vec<Uuid> = row.get("line_brands");
        if source["legalEntityId"].as_str() != Some(legal.to_string().as_str())
            || source[party_key].as_str() != Some(party.to_string().as_str())
            || source["currency"].as_str() != Some(row.get::<String, _>("currency").as_str())
            || !scope.legal_entity_ids.contains(&legal)
            || !scope.business_unit_ids.contains(&unit)
            || !scope.warehouse_ids.contains(&warehouse)
            || brand.is_some_and(|b| !scope.brand_ids.contains(&b))
            || brands.iter().any(|b| !scope.brand_ids.contains(b))
        {
            return Err(StoreError::NotFoundOrForbidden);
        }
        let open: Decimal = row.get("open_amount");
        if row.get::<i64, _>("version") != allocation.expected_version
            || !matches!(
                row.get::<String, _>("status").as_str(),
                "open" | "partially_settled"
            )
            || amount > open
        {
            return Err(StoreError::Conflict);
        }
        total = total
            .checked_add(amount)
            .ok_or_else(|| StoreError::Invalid("allocation total overflow".into()))?;
        let mut target = json!({"id":allocation.document_id,"number":row.get::<String,_>("number"),"version":allocation.expected_version,"amount":amount.to_string(),"openAmount":open.to_string(),"remainingAmount":(open-amount).to_string(),"legalEntityId":legal,"businessUnitId":unit,"warehouseId":warehouse,"brandId":brand});
        target[party_key] = json!(party);
        target["lines"] = json!(brands
            .iter()
            .map(|id| json!({"brandId":id}))
            .collect::<Vec<_>>());
        targets.push(target);
    }
    let available = source["unappliedAmount"]
        .as_str()
        .and_then(|s| s.parse::<Decimal>().ok())
        .ok_or_else(|| StoreError::Invalid("invalid source amount".into()))?;
    if total > available {
        return Err(StoreError::Conflict);
    }
    Ok(
        json!({"source":source,"allocations":targets,"totalAmount":total.to_string(),"remainingUnappliedAmount":(available-total).to_string(),"effect":"按所列金额核销指定应收或应付，减少未核销余额；不发起银行交易","executesBankTransfer":false}),
    )
}
