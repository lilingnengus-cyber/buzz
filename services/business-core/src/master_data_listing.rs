use super::*;

const SQL: &str = r#"WITH authorized AS (SELECT * FROM core_master_data_maintenance WHERE ((resource_type='legal_entity' AND id=ANY($2)) OR (resource_type='business_unit' AND id=ANY($3)) OR resource_type IN ('customer','supplier','warehouse')) AND (resource_type<>'warehouse' OR id=ANY($4)) AND (resource_type<>'customer' OR id=ANY($5)) AND (resource_type<>'supplier' OR id=ANY($6))), filtered AS (SELECT * FROM authorized WHERE ($1::text IS NULL OR resource_type=$1) AND ($7='' OR strpos(lower(concat_ws(' ',code,name,legal_entity_name,business_unit_name,array_to_string(business_unit_path,' '))),lower($7))>0) AND ($8::text IS NULL OR status=$8) AND ($9::uuid IS NULL OR id=$9)) "#;

impl CoreMasterDataService {
    /// Lists matching records using a stable, permission-scoped page.
    pub async fn list_page(
        &self,
        actor: Uuid,
        resource_type: Option<CoreMasterType>,
        limit: i64,
        filter: &crate::master_pagination::MasterPageFilter,
    ) -> Result<CoreMasterList, DomainError> {
        filter.validate()?;
        let snapshot = self.snapshot(actor, "business_master_data:read").await?;
        let entities = snapshot
            .scopes
            .legal_entity_ids
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let units = snapshot
            .scopes
            .business_unit_ids
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let warehouses = snapshot
            .scopes
            .warehouse_ids
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let customers = snapshot
            .scopes
            .customer_ids
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let suppliers = snapshot
            .scopes
            .supplier_ids
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let limit = limit.clamp(1, 1000);
        let mut tx = self.store.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let metadata_sql = format!("{SQL} SELECT (SELECT count(*) FROM filtered), (SELECT jsonb_object_agg(resource_type,n) FROM (SELECT resource_type,count(*) n FROM authorized GROUP BY resource_type) counts)");
        let (total, raw_counts): (i64, Option<serde_json::Value>) =
            sqlx::query_as(sqlx::AssertSqlSafe(metadata_sql.as_str()))
                .bind(resource_type.map(CoreMasterType::as_str))
                .bind(&entities)
                .bind(&units)
                .bind(&warehouses)
                .bind(&customers)
                .bind(&suppliers)
                .bind(filter.query.trim())
                .bind(filter.status.as_deref())
                .bind(filter.id)
                .fetch_one(&mut *tx)
                .await?;
        let items_sql = format!(
            "{SQL} SELECT * FROM filtered ORDER BY CASE resource_type WHEN 'legal_entity' THEN 0 WHEN 'business_unit' THEN 1 WHEN 'customer' THEN 2 WHEN 'supplier' THEN 3 ELSE 4 END,code,id LIMIT $10 OFFSET $11"
        );
        let items = sqlx::query_as::<_, CoreMasterRecord>(sqlx::AssertSqlSafe(items_sql.as_str()))
            .bind(resource_type.map(CoreMasterType::as_str))
            .bind(&entities)
            .bind(&units)
            .bind(&warehouses)
            .bind(&customers)
            .bind(&suppliers)
            .bind(filter.query.trim())
            .bind(filter.status.as_deref())
            .bind(filter.id)
            .bind(limit)
            .bind(filter.offset)
            .fetch_all(&mut *tx)
            .await?;
        tx.commit().await?;
        let counts = raw_counts
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(k, v)| v.as_i64().map(|n| (k, n)))
            .collect();
        Ok(CoreMasterList {
            page: crate::master_pagination::MasterPageMetadata {
                total,
                has_more: filter.offset.saturating_add(items.len() as i64) < total,
                counts,
            },
            items,
            can_manage: snapshot
                .permission_keys
                .contains("business_master_data:manage"),
            data_as_of: chrono::Utc::now(),
        })
    }
}
