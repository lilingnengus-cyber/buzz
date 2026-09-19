use super::*;

pub(super) fn validate_order_input(
    lines: &[SalesOrderLineInput],
    delivery: Option<chrono::NaiveDate>,
    order_date: chrono::NaiveDate,
) -> Result<(), DomainError> {
    if lines.is_empty() || lines.len() > 100 {
        return Err(DomainError::Invalid(
            "sales order requires 1-100 lines".into(),
        ));
    }
    if delivery.is_some_and(|date| date < order_date) {
        return Err(DomainError::Invalid(
            "requestedDeliveryDate precedes orderDate".into(),
        ));
    }
    Ok(())
}

pub(super) fn calculate_lines(
    lines: &[SalesOrderLineInput],
) -> Result<Vec<LineAmount>, DomainError> {
    lines
        .iter()
        .map(|line| {
            let quantity = line
                .quantity
                .positive("quantity")
                .map_err(DomainError::Invalid)?;
            let unit_price = line
                .unit_price
                .non_negative("unitPrice")
                .map_err(DomainError::Invalid)?;
            let discount = line
                .discount_amount
                .non_negative("discountAmount")
                .map_err(DomainError::Invalid)?;
            let tax_rate = line
                .tax_rate
                .non_negative("taxRate")
                .map_err(DomainError::Invalid)?;
            if tax_rate > Decimal::ONE {
                return Err(DomainError::Invalid("taxRate must not exceed 1".into()));
            }
            let subtotal = money(quantity * unit_price);
            if discount > subtotal {
                return Err(DomainError::Invalid(
                    "discount exceeds line subtotal".into(),
                ));
            }
            let net = money(subtotal - discount);
            let tax = money(net * tax_rate);
            Ok(LineAmount {
                quantity,
                unit_price,
                discount,
                net,
                tax_rate,
                tax,
                gross: money(net + tax),
            })
        })
        .collect()
}

pub(super) async fn validate_order_master_data(
    tx: &mut Transaction<'_, Postgres>,
    legal: Uuid,
    customer: Uuid,
    business_unit: Uuid,
    lines: &[SalesOrderLineInput],
) -> Result<Option<i32>, DomainError> {
    sqlx::query("SELECT id FROM business_legal_entities WHERE id=$1 AND status='active' FOR SHARE")
        .bind(legal)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
    // Keep referenced master statuses stable through the order write; a waited-on disable
    // must be observed before inserting a new operational reference.
    let customer_row=sqlx::query("SELECT payment_terms_days FROM business_customers WHERE id=$1 AND legal_entity_id=$2 AND status='active' FOR SHARE").bind(customer).bind(legal).fetch_optional(&mut **tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
    sqlx::query("SELECT id FROM business_units WHERE id=$1 AND legal_entity_id=$2 AND status='active' FOR SHARE")
        .bind(business_unit).bind(legal).fetch_optional(&mut **tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
    for line in lines {
        let row=sqlx::query("SELECT w.legal_entity_id,s.status sku_status,p.base_uom_id,p.brand_id,p.status product_status FROM business_warehouses w,business_skus s JOIN business_products p ON p.id=s.product_id JOIN business_units_of_measure u ON u.id=p.base_uom_id JOIN business_product_categories c ON c.id=p.category_id WHERE w.id=$1 AND s.id=$2 AND w.status='active' AND u.status='active' AND c.status='active' FOR SHARE OF w,s,p,u,c").bind(line.warehouse_id).bind(line.sku_id).fetch_optional(&mut **tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
        if let Some(brand) = row.get::<Option<Uuid>, _>("brand_id") {
            sqlx::query("SELECT id FROM business_brands WHERE id=$1 AND status='active' FOR SHARE")
                .bind(brand)
                .fetch_optional(&mut **tx)
                .await?
                .ok_or(DomainError::NotFoundOrForbidden)?;
        }
        if row.get::<Uuid, _>("legal_entity_id") != legal
            || row.get::<String, _>("sku_status") != "active"
            || row.get::<String, _>("product_status") != "active"
            || row.get::<Uuid, _>("base_uom_id") != line.unit_of_measure_id
            || line
                .brand_id
                .is_some_and(|id| Some(id) != row.get::<Option<Uuid>, _>("brand_id"))
        {
            return Err(DomainError::NotFoundOrForbidden);
        }
    }
    Ok(Some(customer_row.get("payment_terms_days")))
}

pub(super) async fn insert_order_lines(
    tx: &mut Transaction<'_, Postgres>,
    order_id: Uuid,
    header_business_unit: Uuid,
    lines: &[SalesOrderLineInput],
    amounts: &[LineAmount],
) -> Result<(), DomainError> {
    for (index, (line, amount)) in lines.iter().zip(amounts).enumerate() {
        sqlx::query("INSERT INTO sales_order_lines(id,sales_order_id,line_number,sku_id,warehouse_id,unit_of_measure_id,ordered_quantity,unit_price,discount_amount,net_amount,tax_rate,tax_amount,gross_amount,business_unit_id,department_id,brand_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)").bind(Uuid::new_v4()).bind(order_id).bind((index+1)as i32).bind(line.sku_id).bind(line.warehouse_id).bind(line.unit_of_measure_id).bind(amount.quantity).bind(amount.unit_price).bind(amount.discount).bind(amount.net).bind(amount.tax_rate).bind(amount.tax).bind(amount.gross).bind(line.business_unit_id.unwrap_or(header_business_unit)).bind(line.department_id).bind(line.brand_id).execute(&mut **tx).await?;
    }
    Ok(())
}
