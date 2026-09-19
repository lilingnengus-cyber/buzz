use super::*;
use serde_json::Value;
use sqlx::AssertSqlSafe;

/// Version-bound cancellation of a return draft, with a user-supplied reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelReturnDraft {
    /// Current return version from an authorized read.
    pub expected_version: i64,
    /// Cancellation reason, included in the immutable preview and audit event.
    pub reason: String,
}

impl ReturnService {
    pub(crate) async fn cancellation_preview(
        &self,
        actor: Uuid,
        sales: bool,
        id: Uuid,
        command: &Value,
    ) -> Result<Value, DomainError> {
        let mut command: CancelReturnDraft = serde_json::from_value(command.clone())
            .map_err(|_| DomainError::Invalid("invalid return cancellation input".into()))?;
        command.reason = command.reason.trim().to_owned();
        if command.expected_version < 1
            || command.reason.is_empty()
            || command.reason.len() > 500
            || command.reason.chars().any(char::is_control)
        {
            return Err(DomainError::Invalid(
                "positive version and cancellation reason are required".into(),
            ));
        }
        let (
            table,
            orders,
            order_fk,
            party,
            lines,
            return_fk,
            source_lines,
            source_line_fk,
            order_lines,
            order_line_fk,
            source_fk,
        ) = if sales {
            (
                "sales_returns",
                "sales_orders",
                "sales_order_id",
                "customer_id",
                "sales_return_lines",
                "sales_return_id",
                "shipment_lines",
                "shipment_line_id",
                "sales_order_lines",
                "sales_order_line_id",
                "shipment_id",
            )
        } else {
            (
                "purchase_returns",
                "purchase_orders",
                "purchase_order_id",
                "supplier_id",
                "purchase_return_lines",
                "purchase_return_id",
                "goods_receipt_lines",
                "goods_receipt_line_id",
                "purchase_order_lines",
                "purchase_order_line_id",
                "goods_receipt_id",
            )
        };
        super::super::return_scope::check_return(&self.store, actor, sales, id).await?;
        let authority = authorize(
            &self.store,
            actor,
            if sales {
                "shipment:reverse"
            } else {
                "goods_receipt:reverse"
            },
            None,
            None,
            None,
            None,
            None,
        )
        .await?;
        let mut tx = self.store.pool().begin().await?;
        let row = sqlx::query(AssertSqlSafe(format!("SELECT r.id,r.return_number,r.{source_fk} source_id,r.legal_entity_id,r.warehouse_id,r.{party} party_id,r.return_date,r.currency::text currency,r.reason_code,r.business_note,r.status,r.version,o.business_unit_id,o.brand_id FROM {table} r JOIN {orders} o ON o.id=r.{order_fk} WHERE r.id=$1 FOR SHARE OF r")))
            .bind(id).fetch_optional(&mut *tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
        if !authority
            .scopes
            .legal_entity_ids
            .contains(&row.get("legal_entity_id"))
            || !authority
                .scopes
                .warehouse_ids
                .contains(&row.get("warehouse_id"))
            || !(if sales {
                &authority.scopes.customer_ids
            } else {
                &authority.scopes.supplier_ids
            })
            .contains(&row.get("party_id"))
        {
            return Err(DomainError::NotFoundOrForbidden);
        }
        check_draft(&row, command.expected_version)?;
        let rows = sqlx::query(AssertSqlSafe(format!("SELECT l.id,l.{source_line_fk} source_line_id,l.sku_id,l.quantity,ol.brand_id,p.brand_id current_brand_id FROM {lines} l JOIN {source_lines} sl ON sl.id=l.{source_line_fk} JOIN {order_lines} ol ON ol.id=sl.{order_line_fk} JOIN business_skus sku ON sku.id=l.sku_id JOIN business_products p ON p.id=sku.product_id WHERE l.{return_fk}=$1 ORDER BY l.id")))
            .bind(id).fetch_all(&mut *tx).await?;
        let released:Vec<Value> = rows.into_iter().map(|line|json!({"returnLineId":line.get::<Uuid,_>("id"),"sourceLineId":line.get::<Uuid,_>("source_line_id"),"skuId":line.get::<Uuid,_>("sku_id"),"warehouseId":row.get::<Uuid,_>("warehouse_id"),"brandId":line.get::<Option<Uuid>,_>("brand_id"),"currentBrandId":line.get::<Option<Uuid>,_>("current_brand_id"),"returnableQuantityReleased":line.get::<Decimal,_>("quantity").to_string()})).collect();
        let snapshot = json!({"source":{"id":id,"number":row.get::<String,_>("return_number"),"fulfillmentId":row.get::<Uuid,_>("source_id"),"legalEntityId":row.get::<Uuid,_>("legal_entity_id"),"businessUnitId":row.get::<Uuid,_>("business_unit_id"),"warehouseId":row.get::<Uuid,_>("warehouse_id"),"brandId":row.get::<Option<Uuid>,_>("brand_id"),"customerId":if sales {Some(row.get::<Uuid,_>("party_id"))}else{None},"supplierId":if sales {None}else{Some(row.get::<Uuid,_>("party_id"))},"returnDate":row.get::<NaiveDate,_>("return_date"),"currency":row.get::<String,_>("currency"),"reasonCode":row.get::<String,_>("reason_code"),"businessNote":row.get::<Option<String>,_>("business_note"),"status":"draft","version":command.expected_version,"lines":released},"command":command,"lines":[],"effects":{"statusAfter":"cancelled","inventoryQuantityChange":"0","inventoryValueChange":"0","receivableChange":"0","payableChange":"0"}});
        tx.rollback().await?;
        Ok(snapshot)
    }
}
