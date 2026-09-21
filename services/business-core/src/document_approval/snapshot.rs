use super::*;
use serde_json::Value;

/// Read the complete editable order without truncating its lines.
pub(crate) async fn order(
    state: &AppState,
    actor: Uuid,
    kind: &str,
    id: Uuid,
) -> Result<Value, StoreError> {
    match kind {
        "sales_order" => {
            state
                .sales
                .get_order(actor, id)
                .await
                .map_err(|_| StoreError::NotFoundOrForbidden)?;
        }
        "purchase_order" => {
            state
                .purchasing
                .get_order(actor, id)
                .await
                .map_err(|_| StoreError::NotFoundOrForbidden)?;
        }
        _ => return Err(StoreError::NotFoundOrForbidden),
    }
    let (header_sql, lines_sql) = if kind == "sales_order" {
        ("SELECT to_jsonb(o) || jsonb_build_object('subtotal_amount',o.subtotal_amount::text,'discount_amount',o.discount_amount::text,'net_amount',o.net_amount::text,'tax_amount',o.tax_amount::text,'gross_amount',o.gross_amount::text) FROM sales_orders o WHERE id=$1", "SELECT to_jsonb(l) || jsonb_build_object('current_brand_id',p.brand_id,'ordered_quantity',l.ordered_quantity::text,'cancelled_quantity',l.cancelled_quantity::text,'unit_price',l.unit_price::text,'discount_amount',l.discount_amount::text,'net_amount',l.net_amount::text,'tax_rate',l.tax_rate::text,'tax_amount',l.tax_amount::text,'gross_amount',l.gross_amount::text,'reserved_quantity',l.reserved_quantity::text,'shipped_quantity',l.shipped_quantity::text) FROM sales_order_lines l JOIN business_skus sku ON sku.id=l.sku_id JOIN business_products p ON p.id=sku.product_id WHERE sales_order_id=$1 ORDER BY line_number")
    } else {
        ("SELECT to_jsonb(o) || jsonb_build_object('subtotal_amount',o.subtotal_amount::text,'discount_amount',o.discount_amount::text,'net_amount',o.net_amount::text,'tax_amount',o.tax_amount::text,'gross_amount',o.gross_amount::text) FROM purchase_orders o WHERE id=$1", "SELECT to_jsonb(l) || jsonb_build_object('current_brand_id',p.brand_id,'ordered_quantity',l.ordered_quantity::text,'cancelled_quantity',l.cancelled_quantity::text,'unit_price',l.unit_price::text,'discount_amount',l.discount_amount::text,'net_amount',l.net_amount::text,'tax_rate',l.tax_rate::text,'tax_amount',l.tax_amount::text,'gross_amount',l.gross_amount::text,'received_quantity',l.received_quantity::text) FROM purchase_order_lines l JOIN business_skus sku ON sku.id=l.sku_id JOIN business_products p ON p.id=sku.product_id WHERE purchase_order_id=$1 ORDER BY line_number")
    };
    let mut value: Value = sqlx::query_scalar(header_sql)
        .bind(id)
        .fetch_one(state.store.pool())
        .await?;
    let lines: Vec<Value> = sqlx::query_scalar(lines_sql)
        .bind(id)
        .fetch_all(state.store.pool())
        .await?;
    let scope = state.store.snapshot(actor).await?.scopes;
    if lines.iter().any(|line| {
        line["warehouse_id"]
            .as_str()
            .and_then(|id| id.parse::<Uuid>().ok())
            .is_none_or(|id| !scope.warehouse_ids.contains(&id))
            || ["brand_id", "current_brand_id"].iter().any(|key| {
                line[*key]
                    .as_str()
                    .and_then(|id| id.parse::<Uuid>().ok())
                    .is_some_and(|id| !scope.brand_ids.contains(&id))
            })
    }) {
        return Err(StoreError::NotFoundOrForbidden);
    }
    value["lines"] = json!(lines);
    Ok(camel(value))
}

fn camel(value: Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .filter(|(key, _)| {
                    !matches!(
                        key.as_str(),
                        "trace_id" | "created_by_user_id" | "updated_by_user_id"
                    )
                })
                .map(|(key, value)| {
                    let mut parts = key.split('_');
                    let mut name = parts.next().unwrap_or_default().to_owned();
                    for part in parts {
                        let mut chars = part.chars();
                        if let Some(c) = chars.next() {
                            name.extend(c.to_uppercase());
                            name.extend(chars);
                        }
                    }
                    let value = if value.is_number()
                        && (key.ends_with("_amount")
                            || key.ends_with("_quantity")
                            || matches!(key.as_str(), "unit_price" | "tax_rate"))
                    {
                        Value::String(value.to_string())
                    } else {
                        camel(value)
                    };
                    (name, value)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(camel).collect()),
        value => value,
    }
}

pub(super) async fn sales(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> Response {
    match order(&s, c.actor_user_id, "sales_order", id).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => store_error(e, c.trace_id),
    }
}
pub(super) async fn purchase(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> Response {
    match order(&s, c.actor_user_id, "purchase_order", id).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => store_error(e, c.trace_id),
    }
}
