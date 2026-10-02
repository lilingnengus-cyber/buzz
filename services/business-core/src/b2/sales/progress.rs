use super::*;
use serde_json::Value;

impl SalesService {
    /// Reads order progress from authoritative lines and receivables, with per-section access.
    pub async fn order_detail(&self, actor: Uuid, id: Uuid) -> Result<Value, DomainError> {
        let order = self.get_order(actor, id).await?;
        let scope = authorize(
            &self.store,
            actor,
            "sales_order:read",
            Some(order.legal_entity_id),
            None,
            Some(order.customer_id),
            None,
            Some(order.business_unit_id),
        )
        .await?;
        let progress: Value = sqlx::query_scalar(r#"
WITH lines AS (
 SELECT l.*,s.code AS sku_code,s.name AS sku_name,u.name AS unit_name,
 COALESCE(l.brand_id,p.brand_id) AS effective_brand,
 sp.title AS project_title,sp.status AS project_status
 FROM sales_order_lines l JOIN business_skus s ON s.id=l.sku_id
 JOIN business_products p ON p.id=s.product_id
 JOIN business_units_of_measure u ON u.id=l.unit_of_measure_id
 LEFT JOIN service_projects sp ON sp.sales_order_line_id=l.id
 WHERE l.sales_order_id=$1
), visible_lines AS (
 SELECT *, (effective_brand IS NULL OR effective_brand=ANY($2))
 AND (warehouse_id IS NULL OR warehouse_id=ANY($3)) AS allowed FROM lines
), payload AS (
 SELECT *,jsonb_build_object('lineNumber',line_number,'skuCode',sku_code,'name',sku_name,'unit',unit_name,
 'ordered',ordered_quantity::text,'delivered',(shipped_quantity+service_fulfilled_quantity)::text,
 'cancelled',cancelled_quantity::text,'remaining',(ordered_quantity-cancelled_quantity-shipped_quantity-service_fulfilled_quantity)::text,
 'complete',ordered_quantity>cancelled_quantity AND shipped_quantity+service_fulfilled_quantity=ordered_quantity-cancelled_quantity,
 'projectTitle',project_title,'projectStatus',project_status) AS item FROM visible_lines
)
SELECT jsonb_build_object(
 'goods',CASE WHEN $4 AND NOT EXISTS(SELECT 1 FROM payload WHERE service_kind='goods' AND NOT allowed)
 THEN COALESCE((SELECT jsonb_agg(item ORDER BY line_number) FROM payload WHERE service_kind='goods'),'[]'::jsonb) ELSE NULL END,
 'services',CASE WHEN $5 AND NOT EXISTS(SELECT 1 FROM payload WHERE service_kind<>'goods' AND NOT allowed)
 THEN COALESCE((SELECT jsonb_agg(item ORDER BY line_number) FROM payload WHERE service_kind<>'goods'),'[]'::jsonb) ELSE NULL END,
 'payment',CASE WHEN $6 THEN (SELECT jsonb_build_object('receivableCount',count(*),
 'amount',COALESCE(sum(original_amount),0)::text,'settled',COALESCE(sum(settled_amount),0)::text,
 'open',COALESCE(sum(open_amount),0)::text,'overdue',COALESCE(sum(open_amount) FILTER(WHERE due_date<current_date AND open_amount>0),0)::text)
 FROM trade_receivables WHERE sales_order_id=$1 AND status<>'reversed') ELSE NULL END,
 'dataAsOf',current_timestamp)
"#)
            .bind(id)
            .bind(scope.scopes.brand_ids.iter().copied().collect::<Vec<_>>())
            .bind(scope.scopes.warehouse_ids.iter().copied().collect::<Vec<_>>())
            .bind(scope.permission_keys.contains("shipment:read"))
            .bind(scope.permission_keys.contains("service_delivery:read"))
            .bind(scope.permission_keys.contains("receivable:read"))
            .fetch_one(self.store.pool()).await?;
        let mut result = serde_json::to_value(order)?;
        result["progress"] = progress;
        Ok(result)
    }
}
