use super::*;
use sqlx::{Postgres, QueryBuilder};

impl PgStore {
    /// Searches active records after applying every current actor scope, before pagination.
    #[allow(clippy::too_many_arguments)]
    pub async fn search_resources(
        &self,
        resource_type: ResourceType,
        snapshot: &AuthorizationSnapshot,
        query: &str,
        legal_entity_id: Option<Uuid>,
        offset: u32,
        limit: i64,
    ) -> Result<Vec<MasterDataRecord>, StoreError> {
        let mut sql = QueryBuilder::<Postgres>::new("SELECT resource_type,id,code,name,status,CASE WHEN resource_type='business_unit' THEN NULL ELSE legal_entity_id END legal_entity_id,warehouse_id,customer_id,supplier_id,brand_id,business_unit_id,CASE WHEN resource_type='business_unit' THEN (SELECT business_unit_path FROM core_master_data_maintenance tree WHERE tree.resource_type='business_unit' AND tree.id=business_master_data_directory.id) ELSE NULL END ancestor_path,version FROM business_master_data_directory WHERE status='active' AND resource_type=");
        sql.push_bind(resource_type.as_str());
        for (column, values) in [
            ("legal_entity_id", &snapshot.scopes.legal_entity_ids),
            ("warehouse_id", &snapshot.scopes.warehouse_ids),
            ("customer_id", &snapshot.scopes.customer_ids),
            ("supplier_id", &snapshot.scopes.supplier_ids),
            ("brand_id", &snapshot.scopes.brand_ids),
            ("business_unit_id", &snapshot.scopes.business_unit_ids),
        ] {
            if resource_type == ResourceType::BusinessUnit && column == "legal_entity_id" {
                continue;
            }
            sql.push(" AND (")
                .push(column)
                .push(" IS NULL OR ")
                .push(column)
                .push("=ANY(")
                .push_bind(values.iter().copied().collect::<Vec<_>>())
                .push("))");
        }
        if let Some(id) = legal_entity_id.filter(|_| resource_type != ResourceType::BusinessUnit) {
            sql.push(" AND (legal_entity_id IS NULL OR legal_entity_id=")
                .push_bind(id)
                .push(")");
        }
        // strpos treats wildcard characters as literal user text.
        sql.push(" AND (strpos(lower(code),lower(")
            .push_bind(query)
            .push("))>0 OR strpos(lower(name),lower(")
            .push_bind(query)
            .push("))>0) ORDER BY code,id LIMIT ")
            .push_bind(limit.clamp(1, 201))
            .push(" OFFSET ")
            .push_bind(i64::from(offset));
        Ok(sql
            .build_query_as::<MasterDataRecord>()
            .fetch_all(&self.pool)
            .await?)
    }
}
