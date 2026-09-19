use super::*;
use rust_decimal::Decimal;

fn number(line: &Value, key: &str) -> Result<Decimal, StoreError> {
    line[key]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| StoreError::Invalid("invalid quantity".into()))
}
pub(super) async fn snapshot(
    state: &AppState,
    actor: Uuid,
    kind: &str,
    input: &PrepareOrderCancellation,
) -> Result<Value, StoreError> {
    let (source_kind, action) = family(kind)?;
    if input.reason.trim().is_empty()
        || input.reason.chars().count() > 500
        || input.reason.chars().any(char::is_control)
        || input.expected_source_version <= 0
    {
        return Err(StoreError::Invalid(
            "current version and cancellation reason required".into(),
        ));
    }
    let authority = state.store.snapshot(actor).await?;
    if !authority.permission_keys.contains(action) {
        return Err(StoreError::NotFoundOrForbidden);
    }
    let source =
        super::super::snapshot::order(state, actor, source_kind, input.source_document_id).await?;
    if source["version"].as_i64() != Some(input.expected_source_version)
        || !matches!(
            source["lifecycleStatus"].as_str(),
            Some("draft" | "confirmed")
        )
    {
        return Err(StoreError::Conflict);
    }
    let fulfilled_key = if source_kind == "sales_order" {
        "shippedQuantity"
    } else {
        "receivedQuantity"
    };
    let mut cancelled = Decimal::ZERO;
    let mut fulfilled = Decimal::ZERO;
    let mut released = Decimal::ZERO;
    let mut lines = Vec::new();
    for line in source["lines"]
        .as_array()
        .ok_or_else(|| StoreError::Invalid("missing lines".into()))?
    {
        let done = number(line, fulfilled_key)?;
        let remaining =
            number(line, "orderedQuantity")? - done - number(line, "cancelledQuantity")?;
        if remaining < Decimal::ZERO {
            return Err(StoreError::Conflict);
        }
        cancelled = cancelled
            .checked_add(remaining)
            .ok_or(StoreError::Conflict)?;
        fulfilled = fulfilled.checked_add(done).ok_or(StoreError::Conflict)?;
        if source_kind == "sales_order" {
            released = released
                .checked_add(number(line, "reservedQuantity")?)
                .ok_or(StoreError::Conflict)?;
        }
        lines.push(json!({"lineId":line["id"],"skuId":line["skuId"],"warehouseId":line["warehouseId"],"brandId":line["brandId"],"cancelQuantity":remaining.to_string(),"retainedFulfilledQuantity":done.to_string()}));
    }
    if cancelled <= Decimal::ZERO {
        return Err(StoreError::Conflict);
    }
    Ok(
        json!({"source":source,"lines":lines,"reason":input.reason,"cancelQuantity":cancelled.to_string(),"retainedFulfilledQuantity":fulfilled.to_string(),"releasedReservationQuantity":released.to_string(),"resultStatus":if fulfilled==Decimal::ZERO {"cancelled"} else {"completed"},"effect":"取消全部未履约数量，保留已发货或已收货部分及历史；销售订单释放剩余库存预留。不会删除单据、逆转已过账库存或改变已确认往来余额"}),
    )
}
