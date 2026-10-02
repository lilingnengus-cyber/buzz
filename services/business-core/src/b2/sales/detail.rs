use super::*;

impl SalesService {
    /// Fetches one order within the actor's current read permissions and data scopes.
    pub async fn get_order(&self, actor: Uuid, id: Uuid) -> Result<SalesOrderSummary, DomainError> {
        let snapshot = authorize(
            &self.store,
            actor,
            "sales_order:read",
            None,
            None,
            None,
            None,
            None,
        )
        .await?;
        sqlx::query_as::<_, SalesOrderSummary>("SELECT o.id,o.order_number,o.legal_entity_id,le.code AS legal_entity_code,le.name AS legal_entity_name,o.customer_id,c.code AS customer_code,c.name AS customer_name,o.business_unit_id,bu.code AS business_unit_code,bu.name AS business_unit_name,o.currency::text,o.lifecycle_status,o.hold_status,o.fulfillment_status,o.gross_amount,o.order_date,o.updated_at,o.version FROM sales_orders o JOIN business_legal_entities le ON le.id=o.legal_entity_id JOIN business_customers c ON c.id=o.customer_id JOIN business_units bu ON bu.id=o.business_unit_id WHERE NOT EXISTS (SELECT 1 FROM sales_order_events de WHERE de.sales_order_id=o.id AND de.event_type='draft_deleted') AND o.id=$1 AND o.legal_entity_id=ANY($2) AND o.customer_id=ANY($3) AND o.business_unit_id=ANY($4)")
            .bind(id)
            .bind(snapshot.scopes.legal_entity_ids.into_iter().collect::<Vec<_>>())
            .bind(snapshot.scopes.customer_ids.into_iter().collect::<Vec<_>>())
            .bind(snapshot.scopes.business_unit_ids.into_iter().collect::<Vec<_>>())
            .fetch_optional(self.store.pool()).await?
            .ok_or(DomainError::NotFoundOrForbidden)
    }
}

impl SalesService {
    /// Loads a scoped, versioned order and its lines for draft editing.
    pub async fn draft_options(
        &self,
        actor: Uuid,
        id: Uuid,
    ) -> Result<serde_json::Value, DomainError> {
        let snapshot = authorize(
            &self.store,
            actor,
            "sales_order:read",
            None,
            None,
            None,
            None,
            None,
        )
        .await?;
        let mut tx = self.store.pool().begin().await?;
        let row = sqlx::query("SELECT o.* FROM sales_orders o WHERE o.id=$1 AND o.legal_entity_id=ANY($2) AND o.customer_id=ANY($3) AND o.business_unit_id=ANY($4) AND NOT EXISTS (SELECT 1 FROM sales_order_events e WHERE e.sales_order_id=o.id AND e.event_type='draft_deleted') FOR SHARE OF o")
            .bind(id)
            .bind(snapshot.scopes.legal_entity_ids.iter().copied().collect::<Vec<_>>())
            .bind(snapshot.scopes.customer_ids.iter().copied().collect::<Vec<_>>())
            .bind(snapshot.scopes.business_unit_ids.iter().copied().collect::<Vec<_>>())
            .fetch_optional(&mut *tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
        let rows = sqlx::query(
            "SELECT * FROM sales_order_lines WHERE sales_order_id=$1 ORDER BY line_number",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let mut lines = Vec::new();
        for line in rows {
            let warehouse: Option<Uuid> = line.get("warehouse_id");
            if warehouse.is_some_and(|id| !snapshot.scopes.warehouse_ids.contains(&id)) {
                return Err(DomainError::NotFoundOrForbidden);
            }
            lines.push(json!({
                "skuId":line.get::<Uuid,_>("sku_id"), "warehouseId":warehouse,
                "unitOfMeasureId":line.get::<Uuid,_>("unit_of_measure_id"),
                "quantity":line.get::<Decimal,_>("ordered_quantity").to_string(),
                "unitPrice":line.get::<Decimal,_>("unit_price").to_string(),
                "discountAmount":line.get::<Decimal,_>("discount_amount").to_string(),
                "taxRate":line.get::<Decimal,_>("tax_rate").to_string(),
                "businessUnitId":line.get::<Option<Uuid>,_>("business_unit_id"),
                "departmentId":line.get::<Option<Uuid>,_>("department_id"),
                "brandId":line.get::<Option<Uuid>,_>("brand_id")
            }));
        }
        let status: String = row.get("lifecycle_status");
        let result = json!({"canUpdate":status == "draft" && snapshot.permission_keys.contains("sales_order:update_draft"), "draft":{
            "id":id, "orderNumber":row.get::<String,_>("order_number"),
            "legalEntityId":row.get::<Uuid,_>("legal_entity_id"), "customerId":row.get::<Uuid,_>("customer_id"),
            "businessUnitId":row.get::<Uuid,_>("business_unit_id"), "departmentId":row.get::<Option<Uuid>,_>("department_id"),
            "brandId":row.get::<Option<Uuid>,_>("brand_id"), "currency":row.get::<String,_>("currency"),
            "orderDate":row.get::<chrono::NaiveDate,_>("order_date"),
            "requestedDeliveryDate":row.get::<Option<chrono::NaiveDate>,_>("requested_delivery_date"),
            "paymentTermsDays":row.get::<i32,_>("payment_terms_days"),
            "customerReference":row.get::<Option<String>,_>("customer_reference"),
            "businessNote":row.get::<Option<String>,_>("business_note"),
            "lifecycleStatus":status, "version":row.get::<i64,_>("version"), "lines":lines
        }});
        tx.commit().await?;
        Ok(result)
    }
}
