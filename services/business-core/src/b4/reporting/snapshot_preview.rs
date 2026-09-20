//! Shared read-only monthly snapshot preview and guarded execution input.
use super::*;
use sqlx::{Postgres, Transaction};

pub(super) struct SnapshotContent {
    pub(super) watermark: i64,
    pub(super) amounts: Vec<Value>,
    pub(super) data_quality_status: String,
    pub(super) scope_hash: String,
    pub(super) source_hash: String,
    pub(super) existing: Option<sqlx::postgres::PgRow>,
}
impl SnapshotContent {
    pub(super) fn preview(&self, input: &GenerateReportSnapshot, scope: &Value) -> Value {
        json!({"schemaVersion":1,"kind":"management_report_snapshot","input":input,"scope":scope,
            "ruleVersion":"management-profit-v1","scopeHash":self.scope_hash,"sourceWatermark":self.watermark,"sourceHash":self.source_hash,
            "components":self.amounts,"dataQualityStatus":self.data_quality_status,
            "existingSnapshot":self.existing.as_ref().map(|r| json!({"id":r.get::<Uuid,_>("id"),"number":r.get::<String,_>("snapshot_number"),"version":r.get::<i64,_>("version"),"supersedesSnapshotId":r.get::<Option<Uuid>,_>("supersedes_snapshot_id"),"generatedAt":r.get::<chrono::DateTime<Utc>,_>("generated_at"),"dataAsOf":r.get::<chrono::DateTime<Utc>,_>("data_as_of")})),
            "effects":{"createsImmutableSnapshot":self.existing.is_none(),"changesSourceDocuments":false},
            "boundary":"not_statutory_financial_statement"})
    }
}
impl ProfitReportingService {
    /// Preview immutable monthly report content without allocating a number or persisting records.
    pub async fn snapshot_preview(
        &self,
        actor: Uuid,
        input: &GenerateReportSnapshot,
    ) -> Result<Value, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;
        let preview = self.snapshot_preview_on(&mut tx, actor, input).await?;
        tx.commit().await?;
        Ok(preview)
    }
    /// Preview using the caller's repeatable-read transaction without committing it.
    pub async fn snapshot_preview_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        input: &GenerateReportSnapshot,
    ) -> Result<Value, DomainError> {
        let isolation: String = sqlx::query_scalar("SHOW transaction_isolation")
            .fetch_one(&mut **tx)
            .await?;
        if isolation != "repeatable read" && isolation != "serializable" {
            return Err(DomainError::Invalid(
                "snapshot requires repeatable-read isolation".into(),
            ));
        }
        let (_, scope) = self.snapshot_scope_on(tx, actor, input).await?;
        let content = self.snapshot_content_on(tx, input, &scope).await?;
        Ok(content.preview(input, &scope))
    }
    /// Generate only the content bound to the provided preview, rechecking current authority.
    pub async fn generate_snapshot_guarded(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        key: &str,
        input: &GenerateReportSnapshot,
        expected: &Value,
    ) -> Result<CommandResult, DomainError> {
        crate::snapshot_transaction::retry(|| {
            self.generate_snapshot_once(actor, trace_id, key, input, Some(expected))
        })
        .await
    }
    pub(super) async fn snapshot_scope_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        input: &GenerateReportSnapshot,
    ) -> Result<(crate::model::AuthorizationSnapshot, Value), DomainError> {
        period(&input.management_period)?;
        validate_currency(&input.currency)?;
        if !matches!(
            input.report_type.as_str(),
            "management_profit_statement" | "profitability_by_dimension"
        ) {
            return Err(DomainError::Invalid("unsupported report type".into()));
        }
        let auth = crate::master_write_authority::snapshot(
            tx,
            actor,
            "management_report:generate_snapshot",
            false,
        )
        .await?;
        let mut legal_entities = if input.legal_entity_ids.is_empty() {
            auth.scopes
                .legal_entity_ids
                .iter()
                .copied()
                .collect::<Vec<_>>()
        } else {
            if input
                .legal_entity_ids
                .iter()
                .any(|id| !auth.scopes.legal_entity_ids.contains(id))
            {
                return Err(DomainError::NotFoundOrForbidden);
            }
            input.legal_entity_ids.clone()
        };
        legal_entities.sort_unstable();
        legal_entities.dedup();
        let customers = auth.scopes.customer_ids.iter().copied().collect::<Vec<_>>();
        let brands = auth.scopes.brand_ids.iter().copied().collect::<Vec<_>>();
        let business_units = auth
            .scopes
            .business_unit_ids
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let warehouses = auth
            .scopes
            .warehouse_ids
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let scope = json!({"legalEntityIds":legal_entities,"customerIds":customers,"brandIds":brands,"businessUnitIds":business_units,"warehouseIds":warehouses});
        if let Some(prior_id) = input.supersedes_snapshot_id {
            let prior = sqlx::query("SELECT report_type,management_period,currency::text,scope FROM management_report_snapshots WHERE id=$1 FOR SHARE")
                .bind(prior_id).fetch_optional(&mut **tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
            let prior_scope: Value = prior.get("scope");
            if !snapshot_scope_allowed(&prior_scope, &auth.scopes) {
                return Err(DomainError::NotFoundOrForbidden);
            }
            if prior.get::<String, _>("report_type") != input.report_type
                || prior.get::<String, _>("management_period") != input.management_period
                || prior.get::<String, _>("currency") != input.currency
                || prior_scope != scope
            {
                return Err(DomainError::Invalid(
                    "superseded snapshot must have the same report, period, currency and scope"
                        .into(),
                ));
            }
        }
        Ok((auth, scope))
    }
    pub(super) async fn snapshot_content_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        input: &GenerateReportSnapshot,
        scope: &Value,
    ) -> Result<SnapshotContent, DomainError> {
        let legal_entities: Vec<Uuid> = serde_json::from_value(scope["legalEntityIds"].clone())?;
        let customers: Vec<Uuid> = serde_json::from_value(scope["customerIds"].clone())?;
        let brands: Vec<Uuid> = serde_json::from_value(scope["brandIds"].clone())?;
        let business_units: Vec<Uuid> = serde_json::from_value(scope["businessUnitIds"].clone())?;
        let warehouses: Vec<Uuid> = serde_json::from_value(scope["warehouseIds"].clone())?;
        let watermark:i64=sqlx::query_scalar("SELECT COALESCE(max(fact_sequence),0) FROM profit_facts WHERE management_period=$1 AND currency=$2 AND legal_entity_id=ANY($3) AND customer_id=ANY($4) AND (brand_id IS NULL OR brand_id=ANY($5)) AND business_unit_id=ANY($6) AND (warehouse_id IS NULL OR warehouse_id=ANY($7))").bind(&input.management_period).bind(&input.currency).bind(&legal_entities).bind(&customers).bind(&brands).bind(&business_units).bind(&warehouses).fetch_one(&mut **tx).await?;
        let components=sqlx::query("SELECT metric_type,COALESCE(sum(CASE direction WHEN 'normal' THEN amount ELSE -amount END),0)::numeric(24,6) amount,count(*) fact_count FROM profit_facts WHERE management_period=$1 AND currency=$2 AND legal_entity_id=ANY($3) AND fact_sequence<=$4 AND customer_id=ANY($5) AND (brand_id IS NULL OR brand_id=ANY($6)) AND business_unit_id=ANY($7) AND (warehouse_id IS NULL OR warehouse_id=ANY($8)) GROUP BY metric_type ORDER BY metric_type").bind(&input.management_period).bind(&input.currency).bind(&legal_entities).bind(watermark).bind(&customers).bind(&brands).bind(&business_units).bind(&warehouses).fetch_all(&mut **tx).await?;
        let amounts:Vec<Value>=components.into_iter().map(|row|json!({"metricType":row.get::<String,_>("metric_type"),"amount":row.get::<Decimal,_>("amount").to_string(),"factCount":row.get::<i64,_>("fact_count")})).collect();
        let facts_fresh: bool = sqlx::query_scalar("SELECT $1 AND NOT EXISTS(SELECT 1 FROM profit_projection_failures WHERE status='pending') AND COALESCE(max(data_as_of)>=now()-($2*interval '1 minute'),false) FROM profit_facts WHERE management_period=$3 AND currency=$4 AND legal_entity_id=ANY($5) AND customer_id=ANY($6) AND (brand_id IS NULL OR brand_id=ANY($7)) AND business_unit_id=ANY($8) AND (warehouse_id IS NULL OR warehouse_id=ANY($9))")
            .bind(self.worker_enabled).bind(self.stale_after_minutes).bind(&input.management_period).bind(&input.currency).bind(&legal_entities).bind(&customers).bind(&brands).bind(&business_units).bind(&warehouses).fetch_one(&mut **tx).await?;
        let has_unallocated: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM operational_adjustment_lines l JOIN operational_adjustment_batches b ON b.id=l.batch_id WHERE b.management_period=$1 AND b.currency=$2 AND b.status IN ('draft','previewed') AND b.legal_entity_id=ANY($3) AND (l.customer_id IS NULL OR l.customer_id=ANY($4)) AND (l.brand_id IS NULL OR l.brand_id=ANY($5)) AND (l.business_unit_id IS NULL OR l.business_unit_id=ANY($6)) AND (l.warehouse_id IS NULL OR l.warehouse_id=ANY($7)))")
            .bind(&input.management_period).bind(&input.currency).bind(&legal_entities).bind(&customers).bind(&brands).bind(&business_units).bind(&warehouses).fetch_one(&mut **tx).await?;
        let data_quality_status = if facts_fresh && !has_unallocated {
            "complete"
        } else {
            "partial"
        };
        let scope_hash = hex::encode(Sha256::digest(serde_json::to_vec(&scope)?));
        let source_hash = hex::encode(Sha256::digest(serde_json::to_vec(
            &json!({"scope":scope,"watermark":watermark,"amounts":amounts}),
        )?));
        let existing=sqlx::query("SELECT s.id,s.snapshot_number,s.version,s.supersedes_snapshot_id,s.generated_at,s.data_as_of,r.data_quality_status FROM management_report_snapshots s JOIN management_report_snapshot_rows r ON r.snapshot_id=s.id WHERE s.report_type=$1 AND s.management_period=$2 AND s.currency=$3 AND s.scope_hash=$4 AND s.rule_version='management-profit-v1' AND s.source_watermark=$5 AND s.source_hash=$6").bind(&input.report_type).bind(&input.management_period).bind(&input.currency).bind(&scope_hash).bind(watermark).bind(&source_hash).fetch_optional(&mut **tx).await?;
        let data_quality_status = existing
            .as_ref()
            .map(|row| row.get::<String, _>("data_quality_status"))
            .unwrap_or_else(|| data_quality_status.to_owned());
        Ok(SnapshotContent {
            watermark,
            amounts,
            data_quality_status,
            scope_hash,
            source_hash,
            existing,
        })
    }
}
