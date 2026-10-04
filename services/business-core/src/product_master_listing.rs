use super::*;

const SQL: &str = r#"WITH authorized AS (SELECT *, (SELECT p.service_kind FROM business_products p WHERE p.id=CASE WHEN resource_type='product' THEN product_master_data_maintenance.id ELSE product_master_data_maintenance.product_id END) service_kind FROM product_master_data_maintenance WHERE (brand_id IS NULL OR brand_id=ANY($2))), filtered AS (SELECT * FROM authorized WHERE ($1::text IS NULL OR resource_type=$1) AND ($3='' OR strpos(lower(concat_ws(' ',code,name,product_name,category_name,brand_name,barcode)),lower($3))>0) AND ($4::text IS NULL OR status=$4) AND ($5::uuid IS NULL OR id=$5)) "#;

impl ProductMasterService {
    /// Lists matching records using a stable, permission-scoped page.
    pub async fn list_page(
        &self,
        actor: Uuid,
        resource_type: Option<ProductMasterType>,
        limit: i64,
        filter: &crate::master_pagination::MasterPageFilter,
    ) -> Result<ProductMasterList, DomainError> {
        filter.validate()?;
        let snapshot = self.snapshot(actor, "business_product_master:read").await?;
        let brands = snapshot
            .scopes
            .brand_ids
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let limit = limit.clamp(1, 2000);
        let mut tx = self.store.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let metadata_sql = format!("{SQL} SELECT (SELECT count(*) FROM filtered), (SELECT jsonb_object_agg(resource_type,n) FROM (SELECT resource_type,count(*) n FROM authorized GROUP BY resource_type) counts)");
        let (total, raw_counts): (i64, Option<serde_json::Value>) =
            sqlx::query_as(sqlx::AssertSqlSafe(metadata_sql.as_str()))
                .bind(resource_type.map(ProductMasterType::as_str))
                .bind(&brands)
                .bind(filter.query.trim())
                .bind(filter.status.as_deref())
                .bind(filter.id)
                .fetch_one(&mut *tx)
                .await?;
        let items_sql = format!(
            "{SQL} SELECT * FROM filtered ORDER BY CASE resource_type WHEN 'product_category' THEN 0 WHEN 'brand' THEN 1 WHEN 'unit_of_measure' THEN 2 WHEN 'product' THEN 3 WHEN 'sku' THEN 4 ELSE 5 END,code,id LIMIT $6 OFFSET $7"
        );
        let items =
            sqlx::query_as::<_, ProductMasterRecord>(sqlx::AssertSqlSafe(items_sql.as_str()))
                .bind(resource_type.map(ProductMasterType::as_str))
                .bind(&brands)
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
        Ok(ProductMasterList {
            page: crate::master_pagination::MasterPageMetadata {
                total,
                has_more: filter.offset.saturating_add(items.len() as i64) < total,
                counts,
            },
            items,
            can_manage: snapshot
                .permission_keys
                .contains("business_product_master:manage"),
            data_as_of: chrono::Utc::now(),
        })
    }
}
