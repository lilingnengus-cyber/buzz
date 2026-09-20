//! Stable previews and transaction-bound execution for operating snapshots.
use super::*;
pub(super) struct OperatingContent {
    pub(super) period_end: NaiveDate,
    pub(super) period_start_utc: DateTime<Utc>,
    pub(super) period_end_utc: DateTime<Utc>,
    pub(super) scope: Value,
    pub(super) scope_hash: String,
    pub(super) payload: Value,
    pub(super) quality_status: String,
    pub(super) source_hash: String,
    pub(super) existing: Option<sqlx::postgres::PgRow>,
}
impl OperatingContent {
    pub(super) fn preview(&self, actor: Uuid, input: &GenerateOperatingSnapshot) -> Value {
        json!({"schemaVersion":if input.legal_entity_ids.is_some() || input.business_unit_ids.is_some(){2}else{1},"kind":"operating_report_snapshot","input":input,
            "ownerUserId":actor,"periodEnd":self.period_end,
            "periodStartUtc":self.period_start_utc,"periodEndUtc":self.period_end_utc,"timeBasis":"fixed_utc_offset","scope":self.scope,"scopeHash":self.scope_hash,
            "metrics":self.payload,"sourceHash":self.source_hash,"dataQualityStatus":self.quality_status,
            "existingSnapshot":self.existing.as_ref().map(|r| json!({"id":r.get::<Uuid,_>("id"),"generatedAt":r.get::<DateTime<Utc>,_>("generated_at")})),
            "effects":{"createsImmutableSnapshot":self.existing.is_none(),"changesSourceDocuments":false},
            "boundary":"business_operations_only_not_financial_accounting"})
    }
}
impl OperationsService {
    /// Read an operating snapshot preview without writing snapshots, audit or idempotency.
    pub async fn operating_snapshot_preview(
        &self,
        actor: Uuid,
        input: &GenerateOperatingSnapshot,
    ) -> Result<Value, DomainError> {
        crate::snapshot_transaction::retry(|| async {
            let mut tx = self.store.pool().begin().await?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
                .execute(&mut *tx)
                .await?;
            let preview = self
                .operating_snapshot_preview_on(&mut tx, actor, input)
                .await?;
            tx.commit().await?;
            Ok(preview)
        })
        .await
    }
    /// Preview within a caller-owned repeatable-read transaction, without committing it.
    pub async fn operating_snapshot_preview_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        input: &GenerateOperatingSnapshot,
    ) -> Result<Value, DomainError> {
        ensure_isolation(tx).await?;
        Ok(self
            .operating_snapshot_content_on(tx, actor, input)
            .await?
            .preview(actor, input))
    }
    /// Generate only the exact previewed content and recheck current authority.
    pub async fn generate_operating_snapshot_guarded(
        &self,
        actor: Uuid,
        trace: Uuid,
        key: &str,
        input: &GenerateOperatingSnapshot,
        expected: &Value,
    ) -> Result<Value, DomainError> {
        crate::snapshot_transaction::retry(|| {
            self.generate_operating_snapshot_request_once(actor, trace, key, input, Some(expected))
        })
        .await
    }
    /// Join the caller's repeatable-read approval transaction without committing it.
    /// Roll back on every error; retry the whole transaction on serialization failure.
    #[allow(clippy::too_many_arguments)]
    pub async fn generate_operating_snapshot_guarded_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        trace: Uuid,
        key: &str,
        input: &GenerateOperatingSnapshot,
        expected: &Value,
    ) -> Result<Value, DomainError> {
        self.operating_snapshot_request_on(tx, actor, trace, key, input, Some(expected))
            .await
    }
    pub(super) async fn operating_snapshot_content_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        input: &GenerateOperatingSnapshot,
    ) -> Result<OperatingContent, DomainError> {
        validate_snapshot_input(input)?;
        let mut auth = crate::master_write_authority::snapshot(
            tx,
            actor,
            "management_report:generate_snapshot",
            false,
        )
        .await?;
        let period_end = input
            .period_start
            .checked_add_signed(Duration::days(if input.cadence == "daily" { 1 } else { 7 }))
            .ok_or_else(|| {
                DomainError::Invalid("operating period is outside supported dates".into())
            })?;
        let (period_start_utc, period_end_utc) = utc_bounds(input, period_end)?;
        let (selected, scope_hash) = snapshot_scope::resolve(tx, &auth, input).await?;
        let incident_metrics_available = input.business_unit_ids.is_none()
            && selected.legal_entity_ids == auth.scopes.legal_entity_ids;
        let scope = serde_json::to_value(&selected)?;
        auth.scopes = selected;
        let local_today =
            (Utc::now() + Duration::minutes(i64::from(input.utc_offset_minutes))).date_naive();
        if period_end > local_today {
            return Err(DomainError::Invalid(
                "only completed operating periods can be frozen".into(),
            ));
        }
        if let Some(row) = sqlx::query("SELECT id,generated_at,source_hash,data_quality_status,payload,utc_offset_minutes,generated_by_user_id FROM operating_report_snapshots WHERE cadence=$1 AND period_start=$2 AND currency=$3 AND scope_hash=$4 AND utc_offset_minutes=$5")
            .bind(&input.cadence).bind(input.period_start).bind(&input.currency).bind(&scope_hash).bind(input.utc_offset_minutes).fetch_optional(&mut **tx).await?
        {
            return Ok(OperatingContent { period_end, period_start_utc, period_end_utc, scope, scope_hash, payload: row.get("payload"),
                quality_status: row.get("data_quality_status"), source_hash: row.get("source_hash"), existing: Some(row) });
        }
        let le = auth.scopes.legal_entity_ids.into_iter().collect::<Vec<_>>();
        let wh = auth.scopes.warehouse_ids.into_iter().collect::<Vec<_>>();
        let customer = auth.scopes.customer_ids.into_iter().collect::<Vec<_>>();
        let supplier = auth.scopes.supplier_ids.into_iter().collect::<Vec<_>>();
        let brand = auth.scopes.brand_ids.into_iter().collect::<Vec<_>>();
        let bu = auth
            .scopes
            .business_unit_ids
            .into_iter()
            .collect::<Vec<_>>();
        let sales = sqlx::query("SELECT count(*) order_count,COALESCE(sum(gross_amount),0)::numeric(24,6) order_amount FROM sales_orders WHERE order_date>=$1 AND order_date<$2 AND currency=$3 AND legal_entity_id=ANY($4) AND customer_id=ANY($5) AND (brand_id IS NULL OR brand_id=ANY($6)) AND business_unit_id=ANY($7)")
            .bind(input.period_start).bind(period_end).bind(&input.currency).bind(&le).bind(&customer).bind(&brand).bind(&bu).fetch_one(&mut **tx).await?;
        let shipments = sqlx::query("SELECT count(*) FILTER(WHERE s.status='confirmed') shipment_count,COALESCE(sum(s.sales_amount) FILTER(WHERE s.status='confirmed'),0)::numeric(24,6) shipped_revenue FROM shipments s JOIN sales_orders o ON o.id=s.sales_order_id WHERE s.shipment_date>=$1 AND s.shipment_date<$2 AND s.currency=$3 AND s.legal_entity_id=ANY($4) AND s.customer_id=ANY($5) AND s.warehouse_id=ANY($6) AND (o.brand_id IS NULL OR o.brand_id=ANY($7)) AND o.business_unit_id=ANY($8)")
            .bind(input.period_start).bind(period_end).bind(&input.currency).bind(&le).bind(&customer).bind(&wh).bind(&brand).bind(&bu).fetch_one(&mut **tx).await?;
        let purchasing = sqlx::query("SELECT count(*) purchase_order_count,COALESCE(sum(gross_amount),0)::numeric(24,6) purchase_order_amount FROM purchase_orders WHERE order_date>=$1 AND order_date<$2 AND currency=$3 AND legal_entity_id=ANY($4) AND supplier_id=ANY($5) AND (brand_id IS NULL OR brand_id=ANY($6)) AND business_unit_id=ANY($7)")
            .bind(input.period_start).bind(period_end).bind(&input.currency).bind(&le).bind(&supplier).bind(&brand).bind(&bu).fetch_one(&mut **tx).await?;
        let inventory = sqlx::query("SELECT COALESCE(sum(b.inventory_value),0)::numeric(24,6) inventory_value,count(*) FILTER(WHERE b.on_hand_quantity-b.reserved_quantity=0 AND b.reserved_quantity>0) stockout_count FROM inventory_balances b JOIN business_legal_entities e ON e.id=b.legal_entity_id JOIN business_warehouses w ON w.id=b.warehouse_id WHERE e.functional_currency=$1 AND b.legal_entity_id=ANY($2) AND b.warehouse_id=ANY($3) AND ($4::uuid[] IS NULL OR w.business_unit_id=ANY($4))")
            .bind(&input.currency).bind(&le).bind(&wh).bind(input.business_unit_ids.as_ref()).fetch_one(&mut **tx).await?;
        let profit = sqlx::query("SELECT COALESCE(sum(CASE direction WHEN 'normal' THEN amount ELSE -amount END) FILTER(WHERE metric_type='net_revenue'),0)::numeric(24,6) revenue,COALESCE(sum(CASE direction WHEN 'normal' THEN amount ELSE -amount END) FILTER(WHERE metric_type='product_cost'),0)::numeric(24,6) product_cost,COALESCE(sum(CASE direction WHEN 'normal' THEN amount ELSE -amount END) FILTER(WHERE metric_type IN ('outbound_freight','sales_commission','platform_fee','customer_rebate','other_direct_cost','allocated_operating_expense')),0)::numeric(24,6) operating_cost,COALESCE(sum(CASE direction WHEN 'normal' THEN amount ELSE -amount END) FILTER(WHERE metric_type='supplier_rebate'),0)::numeric(24,6) supplier_rebate FROM profit_facts WHERE business_date>=$1 AND business_date<$2 AND currency=$3 AND legal_entity_id=ANY($4) AND customer_id=ANY($5) AND warehouse_id=ANY($6) AND (brand_id IS NULL OR brand_id=ANY($7)) AND business_unit_id=ANY($8)")
            .bind(input.period_start).bind(period_end).bind(&input.currency).bind(&le).bind(&customer).bind(&wh).bind(&brand).bind(&bu).fetch_one(&mut **tx).await?;
        let incidents = sqlx::query("SELECT count(*) FILTER(WHERE first_seen_at >= $2::timestamptz AND first_seen_at < $3::timestamptz) opened_count,count(*) FILTER(WHERE resolved_at >= $2::timestamptz AND resolved_at < $3::timestamptz) resolved_count,count(*) FILTER(WHERE due_at < LEAST(COALESCE(resolved_at,$3::timestamptz),$3::timestamptz) AND first_seen_at < $3::timestamptz) breached_count,COALESCE(avg(EXTRACT(EPOCH FROM (resolved_at-first_seen_at))/3600) FILTER(WHERE resolved_at >= $2::timestamptz AND resolved_at < $3::timestamptz),0)::numeric(18,3) average_resolution_hours FROM operating_report_incidents WHERE scope_hash=$1")
            .bind(&auth.effective_scope_hash).bind(period_start_utc).bind(period_end_utc).fetch_one(&mut **tx).await?;
        let revenue: Decimal = profit.get("revenue");
        let operating_profit = revenue
            - profit.get::<Decimal, _>("product_cost")
            - profit.get::<Decimal, _>("operating_cost")
            + profit.get::<Decimal, _>("supplier_rebate");
        let quality = self
            .data_quality_for_snapshot_on(
                tx,
                actor,
                input.legal_entity_ids.as_ref().map(|_| le.as_slice()),
                input.business_unit_ids.as_ref().map(|_| bu.as_slice()),
            )
            .await?;
        let mut quality_status = quality["status"].as_str().unwrap_or("blocked");
        if !incident_metrics_available && quality_status == "complete" {
            quality_status = "partial";
        }
        let mut payload = json!({
            "salesOrderCount": sales.get::<i64,_>("order_count"),
            "salesOrderAmount": sales.get::<Decimal,_>("order_amount").to_string(),
            "shipmentCount": shipments.get::<i64,_>("shipment_count"),
            "shippedRevenue": shipments.get::<Decimal,_>("shipped_revenue").to_string(),
            "purchaseOrderCount": purchasing.get::<i64,_>("purchase_order_count"),
            "purchaseOrderAmount": purchasing.get::<Decimal,_>("purchase_order_amount").to_string(),
            "inventoryValueAsOfGeneration": inventory.get::<Decimal,_>("inventory_value").to_string(),
            "stockoutCountAsOfGeneration": inventory.get::<i64,_>("stockout_count"),
            "managementOperatingProfit": operating_profit.to_string(),
            "incidentsOpened": incidents.get::<i64,_>("opened_count"),
            "incidentsResolved": incidents.get::<i64,_>("resolved_count"),
            "slaBreached": incidents.get::<i64,_>("breached_count"),
            "averageResolutionHours": incidents.get::<Decimal,_>("average_resolution_hours").to_string()
        });
        if input.legal_entity_ids.is_some() || input.business_unit_ids.is_some() {
            payload["unavailableMetrics"] = json!({});
            if !incident_metrics_available {
                for key in [
                    "incidentsOpened",
                    "incidentsResolved",
                    "slaBreached",
                    "averageResolutionHours",
                ] {
                    payload[key] = Value::Null;
                    payload["unavailableMetrics"][key] =
                        json!(if input.business_unit_ids.is_some() {
                            "not_attributable_to_selected_business_units"
                        } else {
                            "not_attributable_to_selected_legal_entities"
                        });
                }
            }
        }
        let source_hash = hex::encode(Sha256::digest(serde_json::to_vec(&json!({
            "cadence": input.cadence,
            "utcOffsetMinutes": input.utc_offset_minutes,
            "periodStartUtc": period_start_utc,
            "periodEndUtc": period_end_utc,
            "periodStart": input.period_start,
            "periodEnd": period_end,
            "currency": input.currency,
            "scopeHash": scope_hash,
            "metrics": payload
        }))?));
        Ok(OperatingContent {
            period_end,
            period_start_utc,
            period_end_utc,
            scope,
            scope_hash,
            payload,
            quality_status: quality_status.to_owned(),
            source_hash,
            existing: None,
        })
    }
}
pub(super) async fn ensure_isolation(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), DomainError> {
    let isolation: String = sqlx::query_scalar("SHOW transaction_isolation")
        .fetch_one(&mut **tx)
        .await?;
    if isolation != "repeatable read" && isolation != "serializable" {
        return Err(DomainError::Invalid(
            "snapshot requires repeatable-read isolation".into(),
        ));
    }
    Ok(())
}

fn utc_bounds(
    input: &GenerateOperatingSnapshot,
    end: NaiveDate,
) -> Result<(DateTime<Utc>, DateTime<Utc>), DomainError> {
    let convert = |date: NaiveDate| {
        date.and_hms_opt(0, 0, 0)
            .and_then(|value| {
                value.checked_sub_signed(Duration::minutes(i64::from(input.utc_offset_minutes)))
            })
            .map(|value| DateTime::<Utc>::from_naive_utc_and_offset(value, Utc))
            .ok_or_else(|| {
                DomainError::Invalid("operating UTC period is outside supported dates".into())
            })
    };
    Ok((convert(input.period_start)?, convert(end)?))
}
