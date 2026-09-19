use super::*;
use business_query_contracts::SearchFinancialDocumentsInput;
use rust_decimal::Decimal;

fn decimal(value: &Value, key: &str) -> Result<Decimal, StoreError> {
    value[key]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| StoreError::Invalid("invalid balance".into()))
}

pub(super) async fn snapshot(
    state: &AppState,
    actor: Uuid,
    kind: &str,
    input: &PrepareReversal,
) -> Result<Value, StoreError> {
    let (source_kind, action) = family(kind)?;
    if input.reason.trim().is_empty()
        || input.reason.chars().count() > 500
        || input.reason.chars().any(char::is_control)
        || input.expected_source_version <= 0
    {
        return Err(StoreError::Invalid(
            "bounded nonempty reversal reason and current version required".into(),
        ));
    }
    let authority = state.store.snapshot(actor).await?;
    if !authority.permission_keys.contains(action) {
        return Err(StoreError::NotFoundOrForbidden);
    }
    let source = settlement::value(state, actor, source_kind, input.source_document_id).await?;
    if source["version"].as_i64() != Some(input.expected_source_version)
        || !matches!(
            source["status"].as_str(),
            Some("confirmed" | "partially_allocated" | "fully_allocated")
        )
    {
        return Err(StoreError::Conflict);
    }
    if !kind.contains("allocation") {
        if input.allocation_id.is_some() || input.expected_target_version.is_some() {
            return Err(StoreError::Invalid(
                "allocation fields not valid for source reversal".into(),
            ));
        }
        if decimal(&source, "allocatedAmount")? != Decimal::ZERO {
            return Err(StoreError::Invalid("reverse allocations first".into()));
        }
        return Ok(
            json!({"source":source,"reason":input.reason,"resultStatus":"reversed","remainingUnappliedAmount":"0","effect":"逆转业务收付款记录，不发起退款或银行交易","executesBankTransfer":false}),
        );
    }
    let allocation = input
        .allocation_id
        .ok_or_else(|| StoreError::Invalid("allocation required".into()))?;
    let version = input
        .expected_target_version
        .filter(|v| *v > 0)
        .ok_or_else(|| StoreError::Invalid("target version required".into()))?;
    let (sql, target_kind) = if source_kind == "customer_receipt" {
        ("SELECT a.receipt_id source_id,a.receivable_id target_id,a.amount,a.allocation_type,EXISTS(SELECT 1 FROM receivable_allocations r WHERE r.reverses_allocation_id=a.id) reversed FROM receivable_allocations a WHERE a.id=$1","receivable")
    } else {
        ("SELECT a.supplier_payment_id source_id,a.payable_id target_id,a.amount,a.allocation_type,EXISTS(SELECT 1 FROM payable_allocations r WHERE r.reverses_allocation_id=a.id) reversed FROM payable_allocations a WHERE a.id=$1","payable")
    };
    let row = sqlx::query(sql)
        .bind(allocation)
        .fetch_optional(state.store.pool())
        .await?
        .ok_or(StoreError::NotFoundOrForbidden)?;
    if row.get::<Uuid, _>("source_id") != input.source_document_id {
        return Err(StoreError::NotFoundOrForbidden);
    }
    if row.get::<String, _>("allocation_type") != "apply" || row.get::<bool, _>("reversed") {
        return Err(StoreError::Conflict);
    }
    let search = SearchFinancialDocumentsInput {
        document_id: Some(row.get("target_id")),
        query: None,
        party_id: None,
        status: None,
        offset: 0,
        limit: 1,
    };
    let target = super::super::financial_documents::rows(state, actor, target_kind, &search)
        .await?
        .into_iter()
        .next()
        .ok_or(StoreError::NotFoundOrForbidden)?;
    if target["version"].as_i64() != Some(version) || target["status"] == "reversed" {
        return Err(StoreError::Conflict);
    }
    let amount: Decimal = row.get("amount");
    if amount <= Decimal::ZERO
        || decimal(&source, "allocatedAmount")? < amount
        || decimal(&target, "settledAmount")? < amount
    {
        return Err(StoreError::Conflict);
    }
    let unapplied = decimal(&source, "unappliedAmount")?
        .checked_add(amount)
        .ok_or(StoreError::Conflict)?;
    let open = decimal(&target, "openAmount")?
        .checked_add(amount)
        .ok_or(StoreError::Conflict)?;
    Ok(
        json!({"source":source,"target":target,"allocationId":allocation,"amount":amount.to_string(),"reason":input.reason,"remainingUnappliedAmount":unapplied.to_string(),"remainingOpenAmount":open.to_string(),"effect":"逆转指定核销，恢复来源待核销金额及应收应付未结余额；不发起银行交易","executesBankTransfer":false}),
    )
}
