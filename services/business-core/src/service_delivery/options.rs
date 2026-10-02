use super::*;
impl ServiceDelivery {
    /// Return scoped master-data choices and potential responsible users.
    pub async fn options(&self, actor: Uuid) -> Result<Value, DomainError> {
        let scope = self.scope(actor, "service_delivery:read").await?;
        let items:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'name',name,'code',code,'status',status,'resourceType',resource_type,'parentBusinessUnitId',CASE WHEN resource_type='business_unit' THEN (SELECT parent_business_unit_id FROM business_units b WHERE b.id=d.id) ELSE NULL END) FROM business_master_data_directory d WHERE status='active' AND ((resource_type='legal_entity' AND id=ANY($1)) OR (resource_type='business_unit' AND id=ANY($2)) OR (resource_type='customer' AND id=ANY($3))) ORDER BY name,id LIMIT 2001").bind(scope.scopes.legal_entity_ids.iter().copied().collect::<Vec<_>>()).bind(scope.scopes.business_unit_ids.iter().copied().collect::<Vec<_>>()).bind(scope.scopes.customer_ids.iter().copied().collect::<Vec<_>>()).fetch_all(self.store.pool()).await?;
        let users:Vec<(Uuid,String)>=sqlx::query_as("SELECT id,display_name FROM enterprise_users WHERE status='active' ORDER BY display_name,id LIMIT 201").fetch_all(self.store.pool()).await?;
        let mut owners = Vec::new();
        for (id, name) in &users {
            if self.scope(*id, "service_delivery:manage").await.is_ok() {
                owners.push(json!({"id":id,"name":name}));
            }
        }
        Ok(
            json!({"items":items.into_iter().take(2000).collect::<Vec<_>>(),"owners":owners,"currentUserId":actor,"hasMoreOwners":users.len()>200}),
        )
    }
}
