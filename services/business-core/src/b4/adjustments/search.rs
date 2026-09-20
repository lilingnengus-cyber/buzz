//! Paginate visible adjustments without exposing hidden IDs or counts.
use super::*;

/// Narrow adjustment search, ordered by immutable creation time and UUID.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdjustmentSearchQuery {
    /// Optional literal, case-insensitive adjustment-number substring.
    pub number: Option<String>,
    /// Optional authorized legal entity UUID.
    pub legal_entity_id: Option<Uuid>,
    /// Optional management month, YYYY-MM.
    pub management_period: Option<String>,
    /// Optional exact lifecycle status.
    pub status: Option<String>,
    /// Last visible batch ID from the previous response; never a hidden scan position.
    pub after_id: Option<Uuid>,
    /// Page size from 1 to 100; defaults to 20.
    #[serde(default = "default_limit")]
    pub limit: usize,
}
fn default_limit() -> usize {
    20
}
impl AdjustmentService {
    /// Search currently readable batches, applying detail authorization before pagination.
    pub async fn search(
        &self,
        actor: Uuid,
        q: &AdjustmentSearchQuery,
    ) -> Result<Value, DomainError> {
        if q.limit == 0
            || q.limit > 100
            || q.number.as_ref().is_some_and(|s| {
                s.trim().is_empty() || s.chars().count() > 80 || s.chars().any(char::is_control)
            })
            || q.status.as_deref().is_some_and(|s| {
                !matches!(
                    s,
                    "draft" | "previewed" | "posted" | "reversed" | "cancelled"
                )
            })
            || q.management_period.as_deref().is_some_and(|s| {
                s.len() != 7
                    || chrono::NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d").is_err()
            })
            || q.after_id.is_some_and(|id| id.is_nil())
            || q.legal_entity_id.is_some_and(|id| id.is_nil())
        {
            return Err(DomainError::Invalid("invalid adjustment search".into()));
        }
        tokio::time::timeout(std::time::Duration::from_secs(8), self.search_on(actor, q))
            .await
            .map_err(|_| {
                DomainError::Invalid(
                    "adjustment search exceeded time limit; narrow the number, month or status"
                        .into(),
                )
            })?
    }
    async fn search_on(
        &self,
        actor: Uuid,
        q: &AdjustmentSearchQuery,
    ) -> Result<Value, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SET LOCAL statement_timeout='3s'")
            .execute(&mut *tx)
            .await?;
        let authorization = crate::master_write_authority::snapshot(
            &mut tx,
            actor,
            "profit_adjustment:read",
            false,
        )
        .await?;
        if q.legal_entity_id
            .is_some_and(|id| !authorization.scopes.legal_entity_ids.contains(&id))
        {
            return Err(DomainError::NotFoundOrForbidden);
        }
        let detail_query = AdjustmentDetailQuery {
            offset: 0,
            limit: 1,
            expected_version: None,
        };
        if let Some(id) = q.after_id {
            // A removed or revoked anchor must not become an existence oracle.
            self.detail_on(&mut tx, id, &detail_query, &authorization)
                .await?;
        }
        let mut cursor = q.after_id;
        let mut items = Vec::new();
        'scan: loop {
            // Internal scan positions are never returned. Hidden candidates do
            // not consume the visible page size or reveal a hidden total.
            let ids:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM operational_adjustment_batches WHERE legal_entity_id=ANY($1) AND ($2::text IS NULL OR strpos(lower(adjustment_number),lower($2))>0) AND ($3::text IS NULL OR management_period=$3) AND ($4::text IS NULL OR status=$4) AND ($5::uuid IS NULL OR (created_at,id)<(SELECT created_at,id FROM operational_adjustment_batches WHERE id=$5)) AND ($6::uuid IS NULL OR legal_entity_id=$6) ORDER BY created_at DESC,id DESC LIMIT 64")
                .bind(authorization.scopes.legal_entity_ids.iter().copied().collect::<Vec<_>>()).bind(q.number.as_deref()).bind(q.management_period.as_deref()).bind(q.status.as_deref()).bind(cursor).bind(q.legal_entity_id).fetch_all(&mut *tx).await?;
            if ids.is_empty() {
                break;
            }
            for id in ids {
                cursor = Some(id);
                let detail = match self
                    .detail_on(&mut tx, id, &detail_query, &authorization)
                    .await
                {
                    Ok(v) => v,
                    Err(DomainError::NotFoundOrForbidden) => continue,
                    Err(e) => return Err(e),
                };
                let b = &detail["batch"];
                items.push(json!({"id":id,"adjustmentNumber":b["adjustment_number"],"legalEntityId":b["legal_entity_id"],"currency":b["currency"],"managementPeriod":b["management_period"],"status":b["status"],"version":detail["version"],"totalAmount":detail["totalAmount"],"lineCount":detail["pagination"]["total"],"targetOrderCount":detail["targetOrderCount"],"hasUnattributedBrandTargets":detail["hasUnattributedBrandTargets"],"createdAt":b["created_at"],"updatedAt":b["updated_at"]}));
                if items.len() > q.limit {
                    break 'scan;
                }
            }
        }
        let has_more = items.len() > q.limit;
        items.truncate(q.limit);
        let next = if has_more {
            items.last().map(|v| v["id"].clone())
        } else {
            None
        };
        let result = json!({"schemaVersion":1,"items":items,"scope":authorization.scopes,"pagination":{"limit":q.limit,"hasMore":has_more,"nextAfterId":next},"dataAsOf":Utc::now(),"boundary":"management_only_not_general_ledger"});
        tx.commit().await?;
        Ok(result)
    }
}
