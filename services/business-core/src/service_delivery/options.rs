use super::*;
impl ServiceDelivery {
    /// Return scoped master-data choices and potential responsible users.
    pub async fn options(&self, actor: Uuid, query: Option<&str>) -> Result<Value, DomainError> {
        let scope = self.scope(actor, "service_delivery:read").await?;
        let items:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'name',name,'code',code,'status',status,'resourceType',resource_type,'parentBusinessUnitId',CASE WHEN resource_type='business_unit' THEN (SELECT parent_business_unit_id FROM business_units b WHERE b.id=d.id) ELSE NULL END) FROM business_master_data_directory d WHERE status='active' AND ((resource_type='legal_entity' AND id=ANY($1)) OR (resource_type='business_unit' AND id=ANY($2)) OR (resource_type='customer' AND id=ANY($3))) ORDER BY name,id LIMIT 2001").bind(scope.scopes.legal_entity_ids.iter().copied().collect::<Vec<_>>()).bind(scope.scopes.business_unit_ids.iter().copied().collect::<Vec<_>>()).bind(scope.scopes.customer_ids.iter().copied().collect::<Vec<_>>()).fetch_all(self.store.pool()).await?;
        let users:Vec<(Uuid,String)>=sqlx::query_as("SELECT id,display_name FROM enterprise_users WHERE status='active' ORDER BY display_name,id LIMIT 201").fetch_all(self.store.pool()).await?;
        let mut owners = Vec::new();
        for (id, name) in &users {
            if self.scope(*id, "service_delivery:manage").await.is_ok() {
                owners.push(json!({"id":id,"name":name}));
            }
        }
        model::text(query.unwrap_or(""), 160, false)?;
        let order_lines = if let Ok(sales) = self.scope(actor, "sales_order:read").await {
            sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',l.id,'order_number',o.order_number,'title',s.name,'customer_name',c.name,'customer_id',o.customer_id,'legal_entity_id',o.legal_entity_id,'business_unit_id',o.business_unit_id,'service_kind',l.service_kind,'amount',l.gross_amount::text,'currency',o.currency) FROM sales_order_lines l JOIN sales_orders o ON o.id=l.sales_order_id JOIN business_skus s ON s.id=l.sku_id JOIN business_products p ON p.id=s.product_id JOIN business_customers c ON c.id=o.customer_id WHERE o.lifecycle_status='confirmed' AND o.hold_status='none' AND l.service_kind<>'goods' AND l.cancelled_quantity=0 AND l.service_fulfilled_quantity=0 AND NOT EXISTS(SELECT 1 FROM service_projects sp WHERE sp.sales_order_line_id=l.id) AND o.legal_entity_id=ANY($1) AND o.business_unit_id=ANY($2) AND o.customer_id=ANY($3) AND (COALESCE(l.brand_id,p.brand_id) IS NULL OR COALESCE(l.brand_id,p.brand_id)=ANY($4)) AND strpos(lower(o.order_number||' '||s.name||' '||c.name),lower($5))>0 ORDER BY o.order_date DESC,o.id,l.line_number LIMIT 101")
                .bind(sales.scopes.legal_entity_ids.intersection(&scope.scopes.legal_entity_ids).copied().collect::<Vec<_>>())
                .bind(sales.scopes.business_unit_ids.intersection(&scope.scopes.business_unit_ids).copied().collect::<Vec<_>>())
                .bind(sales.scopes.customer_ids.intersection(&scope.scopes.customer_ids).copied().collect::<Vec<_>>())
                .bind(sales.scopes.brand_ids.iter().copied().collect::<Vec<_>>())
                .bind(query.unwrap_or("")).fetch_all(self.store.pool()).await?
        } else {
            vec![]
        };
        let has_more_orders = order_lines.len() > 100;
        Ok(
            json!({"items":items.into_iter().take(2000).collect::<Vec<_>>(),"owners":owners,"currentUserId":actor,"hasMoreOwners":users.len()>200,"orderLines":order_lines.into_iter().take(100).collect::<Vec<_>>(),"hasMoreOrders":has_more_orders}),
        )
    }
}
