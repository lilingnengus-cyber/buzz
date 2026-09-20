//! Transaction-bound data quality aggregation.
use super::*;

impl OperationsService {
    pub async fn data_quality(&self, actor: Uuid) -> Result<Value, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;
        let result = self.data_quality_on(&mut tx, actor).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub(super) async fn data_quality_on(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        actor: Uuid,
    ) -> Result<Value, DomainError> {
        let overall_started = Instant::now();
        let mut stages = Vec::with_capacity(8);
        let stage_started = Instant::now();
        let auth = crate::master_write_authority::read(tx, actor, "management_report:read").await?;
        record_stage(&mut stages, "authorization", stage_started);
        let legal_entities = auth.scopes.legal_entity_ids.into_iter().collect::<Vec<_>>();
        let warehouses = auth.scopes.warehouse_ids.into_iter().collect::<Vec<_>>();
        let customers = auth.scopes.customer_ids.into_iter().collect::<Vec<_>>();
        let suppliers = auth.scopes.supplier_ids.into_iter().collect::<Vec<_>>();
        let brands = auth.scopes.brand_ids.into_iter().collect::<Vec<_>>();
        let business_units = auth
            .scopes
            .business_unit_ids
            .into_iter()
            .collect::<Vec<_>>();

        let stage_started = Instant::now();
        let inventory: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM inventory_balance_reconciliation WHERE legal_entity_id=ANY($1) AND warehouse_id=ANY($2) AND (on_hand_difference<>0 OR reserved_difference<>0 OR value_difference<>0)",
        )
        .bind(&legal_entities)
        .bind(&warehouses)
        .fetch_one(&mut **tx)
        .await?;
        record_stage(&mut stages, "inventoryReconciliation", stage_started);
        let stage_started = Instant::now();
        let receivables: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM receivable_balance_reconciliation r JOIN trade_receivables t ON t.id=r.receivable_id WHERE t.legal_entity_id=ANY($1) AND t.customer_id=ANY($2) AND (r.settled_difference<>0 OR r.open_difference<>0)",
        )
        .bind(&legal_entities)
        .bind(&customers)
        .fetch_one(&mut **tx)
        .await?;
        record_stage(&mut stages, "receivablesReconciliation", stage_started);
        let stage_started = Instant::now();
        let payables: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM payable_balance_reconciliation r JOIN trade_payables p ON p.id=r.payable_id WHERE p.legal_entity_id=ANY($1) AND p.supplier_id=ANY($2) AND (r.settled_difference<>0 OR r.open_difference<>0)",
        )
        .bind(&legal_entities)
        .bind(&suppliers)
        .fetch_one(&mut **tx)
        .await?;
        record_stage(&mut stages, "payablesReconciliation", stage_started);
        let stage_started = Instant::now();
        let profit: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM profit_projection_reconciliation r JOIN shipments s ON s.id=r.shipment_id JOIN sales_orders o ON o.id=r.sales_order_id WHERE s.legal_entity_id=ANY($1) AND s.customer_id=ANY($2) AND s.warehouse_id=ANY($3) AND (o.brand_id IS NULL OR o.brand_id=ANY($4)) AND o.business_unit_id=ANY($5) AND (r.revenue_difference<>0 OR r.cost_difference<>0)",
        )
        .bind(&legal_entities)
        .bind(&customers)
        .bind(&warehouses)
        .bind(&brands)
        .bind(&business_units)
        .fetch_one(&mut **tx)
        .await?;
        record_stage(&mut stages, "profitReconciliation", stage_started);
        let stage_started = Instant::now();
        let projection = sqlx::query(sqlx::AssertSqlSafe(PROJECTION_HEALTH_SQL))
            .bind(&legal_entities)
            .bind(&customers)
            .bind(&warehouses)
            .bind(&brands)
            .bind(&business_units)
            .fetch_optional(&mut **tx)
            .await?;
        record_stage(&mut stages, "projectionHealth", stage_started);
        let pending_failures = projection
            .as_ref()
            .map_or(0, |row| row.get::<i64, _>("pending_failures"));
        let pending_events = projection
            .as_ref()
            .map_or(0, |row| row.get::<i64, _>("pending_events"));
        let updated_at = projection
            .as_ref()
            .and_then(|row| row.get::<Option<chrono::DateTime<Utc>>, _>("updated_at"));
        let now = Utc::now();
        let freshness_age_seconds =
            updated_at.map(|value| now.signed_duration_since(value).num_seconds().max(0));
        let stale_after_seconds = self.projection_stale_after_minutes.saturating_mul(60);
        let projection_fresh = self.projection_worker_enabled
            && freshness_age_seconds.is_some_and(|value| value <= stale_after_seconds);
        let difference_count = inventory + receivables + payables + profit;
        let status = if difference_count > 0 || pending_failures > 0 {
            "blocked"
        } else if pending_events > 0 || !projection_fresh {
            "partial"
        } else {
            "complete"
        };
        let duration_ms = elapsed_ms(overall_started);
        let mut alerts = Vec::new();
        for (domain, count, evidence_path) in [
            ("inventory", inventory, "/api/v1/reconciliation/inventory"),
            (
                "receivables",
                receivables,
                "/api/v1/reconciliation/receivables",
            ),
            ("payables", payables, "/api/v1/reconciliation/payables"),
            ("profitFacts", profit, "/api/v1/reconciliation/profit-facts"),
        ] {
            if count > 0 {
                alerts.push(alert(
                    "RECONCILIATION_DIFFERENCE",
                    "critical",
                    &format!("{domain} 存在 {count} 条对账差异"),
                    evidence_path,
                ));
            }
        }
        append_projection_alerts(
            &mut alerts,
            self.projection_worker_enabled,
            projection_fresh,
            pending_events,
            pending_failures,
        );
        if duration_ms > DATA_QUALITY_TARGET_MS {
            alerts.push(alert(
                "SLOW_REPORT_READ",
                "warning",
                "数据质量聚合超过 2 秒目标",
                "/api/v1/operations/data-quality",
            ));
        }
        Ok(json!({
            "status": status,
            "differenceCount": difference_count,
            "checks": [
                check("inventory", inventory, "/api/v1/reconciliation/inventory"),
                check("receivables", receivables, "/api/v1/reconciliation/receivables"),
                check("payables", payables, "/api/v1/reconciliation/payables"),
                check("profitFacts", profit, "/api/v1/reconciliation/profit-facts")
            ],
            "projection": {
                "workerEnabled": self.projection_worker_enabled,
                "fresh": projection_fresh,
                "pendingEvents": pending_events,
                "pendingFailures": pending_failures,
                "freshnessAgeSeconds": freshness_age_seconds,
                "staleAfterSeconds": stale_after_seconds,
                "lastOutboxCreatedAt": projection.as_ref().and_then(|row| row.get::<Option<chrono::DateTime<Utc>>,_>("last_outbox_created_at")),
                "lastFactSequence": projection.as_ref().and_then(|row| row.get::<Option<i64>,_>("last_fact_sequence")),
                "updatedAt": updated_at
            },
            "alerts": alerts,
            "diagnostics": read_diagnostics(stages, duration_ms, DATA_QUALITY_TARGET_MS),
            "scopeVersion": auth.scope_version,
            "effectiveScopeHash": auth.effective_scope_hash,
            "dataAsOf": Utc::now(),
            "repairPolicy": "inspect_evidence_then_run_scoped_reconciliation_or_idempotent_projection_replay",
            "boundary": "business_operations_only_not_financial_accounting"
        }))
    }
}
