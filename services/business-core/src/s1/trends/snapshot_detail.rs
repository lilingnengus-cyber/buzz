use super::*;
use sqlx::postgres::PgRow;
/// Compare recorded report scope, not the changing authorization revision.
pub(super) fn same_scope(left: &PgRow, right: &PgRow) -> bool {
    // Explicit business-unit selection also changes inventory and quality attribution.
    let unit_filtered = |row: &PgRow| {
        row.get::<Value, _>("payload")["unavailableMetrics"]["slaBreached"]
            == "not_attributable_to_selected_business_units"
    };
    if unit_filtered(left) != unit_filtered(right) {
        return false;
    }
    match (
        left.get::<Option<Value>, _>("snapshot_scope"),
        right.get::<Option<Value>, _>("snapshot_scope"),
    ) {
        (Some(a), Some(b)) => a == b,
        (None, None) => left.get::<String, _>("scope_hash") == right.get::<String, _>("scope_hash"),
        _ => false,
    }
}
impl OperationsService {
    /// Read an immutable operating report using current permission and its recorded scope.
    /// Legacy rows without a scope remain bound to the exact original authorization identity.
    pub async fn operating_snapshot_detail(
        &self,
        actor: Uuid,
        id: Uuid,
    ) -> Result<Value, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;
        let auth = crate::master_write_authority::snapshot(
            &mut tx,
            actor,
            "management_report:read",
            false,
        )
        .await?;
        let scopes = serde_json::to_value(&auth.scopes)?;
        let row=sqlx::query("SELECT id,cadence,period_start,period_end,currency::text,payload,data_quality_status,source_hash,generated_at,generated_by_user_id,trace_id,utc_offset_minutes,snapshot_scope FROM operating_report_snapshots WHERE id=$1 AND ((snapshot_scope IS NULL AND scope_hash=$2) OR (snapshot_scope IS NOT NULL AND $3::jsonb @> snapshot_scope))")
            .bind(id).bind(&auth.effective_scope_hash).bind(scopes).fetch_optional(&mut *tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
        let offset: Option<i16> = row.get("utc_offset_minutes");
        let scope: Option<Value> = row.get("snapshot_scope");
        let value = json!({"id":row.get::<Uuid,_>("id"),"cadence":row.get::<String,_>("cadence"),"periodStart":row.get::<NaiveDate,_>("period_start"),"periodEnd":row.get::<NaiveDate,_>("period_end"),"currency":row.get::<String,_>("currency"),"metrics":row.get::<Value,_>("payload"),"dataQualityStatus":row.get::<String,_>("data_quality_status"),"sourceHash":row.get::<String,_>("source_hash"),"generatedAt":row.get::<DateTime<Utc>,_>("generated_at"),"ownerUserId":row.get::<Uuid,_>("generated_by_user_id"),"traceId":row.get::<Uuid,_>("trace_id"),"utcOffsetMinutes":offset,"timeBasis":if offset.is_some(){"fixed_utc_offset"}else{"legacy_unknown"},"scope":scope,"scopeBasis":if scope.is_some(){"recorded"}else{"legacy_current_identity"},"boundary":"business_operations_only_not_financial_accounting"});
        tx.commit().await?;
        Ok(value)
    }
}
