mod snapshot_detail;
mod snapshot_preview;
use super::OperationsService;
use crate::{
    b2::{
        common::{authorize, begin_idempotent, finish_idempotent, request_hash},
        DomainError,
    },
    store::audit,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc, Weekday};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use snapshot_preview::OperatingContent;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

const SUBSCRIPTION_PERMISSION: &str = "management_report:manage_subscriptions";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerateOperatingSnapshot {
    pub cadence: String,
    pub currency: String,
    pub period_start: NaiveDate,
    pub utc_offset_minutes: i16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateSubscription {
    pub cadence: String,
    pub currency: String,
    pub utc_offset_minutes: i16,
    pub delivery_hour: i16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionCommand {
    pub action: String,
    pub expected_version: i64,
}

impl OperationsService {
    pub async fn operating_trends(
        &self,
        actor: Uuid,
        cadence: &str,
        currency: &str,
        limit: i64,
    ) -> Result<Value, DomainError> {
        validate_cadence(cadence)?;
        crate::b2::common::validate_currency(currency)?;
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
        let rows = sqlx::query("SELECT id,cadence,period_start,period_end,currency::text,payload,data_quality_status,source_hash,generated_at,trace_id,utc_offset_minutes,snapshot_scope,scope_hash FROM operating_report_snapshots WHERE ((snapshot_scope IS NULL AND scope_hash=$1) OR (snapshot_scope IS NOT NULL AND generated_by_user_id=$5 AND $6::jsonb @> snapshot_scope)) AND cadence=$2 AND currency=$3 ORDER BY period_start DESC,generated_at DESC,id DESC LIMIT $4")
            .bind(&auth.effective_scope_hash).bind(cadence).bind(currency).bind(limit.clamp(2,60)).bind(actor).bind(scopes).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        let payloads = rows
            .iter()
            .map(|row| row.get::<Value, _>("payload"))
            .collect::<Vec<_>>();
        let items = rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let payload = payloads[index].clone();
                let offset: Option<i16> = row.get("utc_offset_minutes");
                let comparison = offset.and_then(|offset| rows.iter().enumerate().skip(index + 1)
                    .find(|(_, prior)| prior.get::<Option<i16>, _>("utc_offset_minutes") == Some(offset)
                        && prior.get::<NaiveDate, _>("period_start") < row.get::<NaiveDate, _>("period_start")
                        && snapshot_detail::same_scope(row, prior)));

                json!({
                    "id": row.get::<Uuid,_>("id"),
                    "cadence": row.get::<String,_>("cadence"),
                    "periodStart": row.get::<NaiveDate,_>("period_start"),
                    "periodEnd": row.get::<NaiveDate,_>("period_end"),
                    "currency": row.get::<String,_>("currency"),
                    "metrics": payload,
                    "utcOffsetMinutes": offset,
                    "timeBasis": if offset.is_some() { "fixed_utc_offset" } else { "legacy_unknown" },
                    "comparisonSnapshotId": comparison.map(|(_, prior)| prior.get::<Uuid,_>("id")),
                    "change": comparison.map(|(i, _)| trend_change(&payload, &payloads[i])),
                    "dataQualityStatus": row.get::<String,_>("data_quality_status"),
                    "sourceHash": row.get::<String,_>("source_hash"),
                    "generatedAt": row.get::<DateTime<Utc>,_>("generated_at"),
                    "traceId": row.get::<Uuid,_>("trace_id")
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({
            "items": items,
            "cadence": cadence,
            "currency": currency,
            "scopeVersion": auth.scope_version,
            "effectiveScopeHash": auth.effective_scope_hash,
            "dataAsOf": Utc::now(),
            "boundary": "business_operations_only_not_financial_accounting"
        }))
    }

    pub async fn generate_operating_snapshot(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        idempotency_key: &str,
        input: &GenerateOperatingSnapshot,
    ) -> Result<Value, DomainError> {
        crate::snapshot_transaction::retry(|| {
            self.generate_operating_snapshot_request_once(
                actor,
                trace_id,
                idempotency_key,
                input,
                None,
            )
        })
        .await
    }

    async fn generate_operating_snapshot_request_once(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        idempotency_key: &str,
        input: &GenerateOperatingSnapshot,
        expected: Option<&Value>,
    ) -> Result<Value, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;
        let result = self
            .operating_snapshot_request_on(
                &mut tx,
                actor,
                trace_id,
                idempotency_key,
                input,
                expected,
            )
            .await?;
        tx.commit().await?;
        Ok(result)
    }
    async fn operating_snapshot_request_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        trace_id: Uuid,
        idempotency_key: &str,
        input: &GenerateOperatingSnapshot,
        expected: Option<&Value>,
    ) -> Result<Value, DomainError> {
        snapshot_preview::ensure_isolation(tx).await?;
        validate_snapshot_input(input)?;
        let hash = match expected {
            Some(preview) => request_hash(&("guarded-operating-snapshot-v1", input, preview))?,
            None => request_hash(input)?,
        };
        let auth = crate::master_write_authority::snapshot(
            tx,
            actor,
            "management_report:generate_snapshot",
            false,
        )
        .await?;
        if let Some(value) = begin_idempotent::<Value>(
            tx,
            actor,
            "operating_snapshot_generate",
            idempotency_key,
            &hash,
        )
        .await?
        {
            let id = value["id"]
                .as_str()
                .and_then(|id| Uuid::parse_str(id).ok())
                .ok_or(DomainError::NotFoundOrForbidden)?;
            let visible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM operating_report_snapshots WHERE id=$1 AND scope_hash=$2)")
                .bind(id).bind(&auth.effective_scope_hash).fetch_one(&mut **tx).await?;
            if !visible {
                return Err(DomainError::NotFoundOrForbidden);
            }
            return Ok(value);
        }
        let result = self
            .generate_operating_snapshot_on(tx, actor, trace_id, input, expected)
            .await?;
        finish_idempotent(
            tx,
            actor,
            "operating_snapshot_generate",
            idempotency_key,
            &result,
        )
        .await?;
        Ok(result)
    }

    async fn generate_operating_snapshot_once(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        input: &GenerateOperatingSnapshot,
    ) -> Result<Value, DomainError> {
        crate::snapshot_transaction::retry(|| {
            self.generate_operating_snapshot_unkeyed_once(actor, trace_id, input)
        })
        .await
    }

    async fn generate_operating_snapshot_unkeyed_once(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        input: &GenerateOperatingSnapshot,
    ) -> Result<Value, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;
        let result = self
            .generate_operating_snapshot_on(&mut tx, actor, trace_id, input, None)
            .await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn generate_operating_snapshot_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        trace_id: Uuid,
        input: &GenerateOperatingSnapshot,
        expected: Option<&Value>,
    ) -> Result<Value, DomainError> {
        let content = self.operating_snapshot_content_on(tx, actor, input).await?;
        if expected.is_some_and(|preview| *preview != content.preview(actor, input)) {
            return Err(DomainError::StalePreview);
        }
        let OperatingContent {
            period_end,
            scope_hash,
            scope,
            payload,
            quality_status,
            source_hash,
            existing,
            ..
        } = content;
        if let Some(row) = existing {
            return Ok(snapshot_result(&row, false, trace_id));
        }
        let id = Uuid::new_v4();
        let inserted = sqlx::query("INSERT INTO operating_report_snapshots(id,cadence,period_start,period_end,currency,scope_hash,payload,data_quality_status,source_hash,generated_by_user_id,trace_id,utc_offset_minutes,snapshot_scope) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) ON CONFLICT(cadence,period_start,currency,scope_hash,utc_offset_minutes) WHERE utc_offset_minutes IS NOT NULL DO NOTHING RETURNING id,generated_at,source_hash,data_quality_status,utc_offset_minutes,generated_by_user_id")
            .bind(id).bind(&input.cadence).bind(input.period_start).bind(period_end).bind(&input.currency).bind(&scope_hash).bind(&payload).bind(&quality_status).bind(&source_hash).bind(actor).bind(trace_id).bind(input.utc_offset_minutes).bind(scope).fetch_optional(&mut **tx).await?;
        let (row, created) = if let Some(row) = inserted {
            audit(tx, trace_id, actor, "operating_snapshot.generate", "operating_report_snapshot", &id.to_string(), json!({"cadence":input.cadence,"periodStart":input.period_start,"periodEnd":period_end,"currency":input.currency,"utcOffsetMinutes":input.utc_offset_minutes,"sourceHash":source_hash})).await?;
            (row, true)
        } else {
            (sqlx::query("SELECT id,generated_at,source_hash,data_quality_status,utc_offset_minutes,generated_by_user_id FROM operating_report_snapshots WHERE cadence=$1 AND period_start=$2 AND currency=$3 AND scope_hash=$4 AND utc_offset_minutes=$5").bind(&input.cadence).bind(input.period_start).bind(&input.currency).bind(&scope_hash).bind(input.utc_offset_minutes).fetch_one(&mut **tx).await?, false)
        };
        Ok(snapshot_result(&row, created, trace_id))
    }

    pub async fn list_operating_subscriptions(&self, actor: Uuid) -> Result<Value, DomainError> {
        authorize(
            &self.store,
            actor,
            "management_report:read",
            None,
            None,
            None,
            None,
            None,
        )
        .await?;
        let rows = sqlx::query("SELECT id,cadence,currency::text,utc_offset_minutes,delivery_hour,status,next_run_at,last_run_at,last_snapshot_id,version FROM operating_report_subscriptions WHERE owner_user_id=$1 ORDER BY cadence,currency")
            .bind(actor).fetch_all(self.store.pool()).await?;
        Ok(
            json!({"items":rows.iter().map(subscription_json).collect::<Vec<_>>(),"dataAsOf":Utc::now(),"boundary":"in_dock_operating_snapshots_only"}),
        )
    }

    pub async fn create_operating_subscription(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        key: &str,
        input: &CreateSubscription,
    ) -> Result<Value, DomainError> {
        validate_subscription(input)?;
        authorize(
            &self.store,
            actor,
            SUBSCRIPTION_PERMISSION,
            None,
            None,
            None,
            None,
            None,
        )
        .await?;
        let hash = request_hash(input)?;
        let mut tx = self.store.pool().begin().await?;
        if let Some(value) =
            begin_idempotent::<Value>(&mut tx, actor, "operating_subscription_create", key, &hash)
                .await?
        {
            tx.commit().await?;
            return Ok(value);
        }
        let id = Uuid::new_v4();
        let next_run = next_run_at(
            &input.cadence,
            input.utc_offset_minutes,
            input.delivery_hour,
            Utc::now(),
        );
        let row = sqlx::query("INSERT INTO operating_report_subscriptions(id,owner_user_id,cadence,currency,utc_offset_minutes,delivery_hour,next_run_at) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(owner_user_id,cadence,currency) DO UPDATE SET utc_offset_minutes=EXCLUDED.utc_offset_minutes,delivery_hour=EXCLUDED.delivery_hour,next_run_at=EXCLUDED.next_run_at,status='active',version=operating_report_subscriptions.version+1 RETURNING id,cadence,currency::text,utc_offset_minutes,delivery_hour,status,next_run_at,last_run_at,last_snapshot_id,version")
            .bind(id).bind(actor).bind(&input.cadence).bind(&input.currency).bind(input.utc_offset_minutes).bind(input.delivery_hour).bind(next_run).fetch_one(&mut *tx).await?;
        let subscription_id: Uuid = row.get("id");
        subscription_event(
            &mut tx,
            subscription_id,
            "created",
            actor,
            trace_id,
            json!({"nextRunAt":next_run}),
        )
        .await?;
        audit(
            &mut tx,
            trace_id,
            actor,
            "operating_subscription.save",
            "operating_report_subscription",
            &subscription_id.to_string(),
            json!({"cadence":input.cadence,"currency":input.currency,"nextRunAt":next_run}),
        )
        .await?;
        let result = subscription_json(&row);
        finish_idempotent(
            &mut tx,
            actor,
            "operating_subscription_create",
            key,
            &result,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn command_operating_subscription(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        key: &str,
        id: Uuid,
        input: &SubscriptionCommand,
    ) -> Result<Value, DomainError> {
        authorize(
            &self.store,
            actor,
            SUBSCRIPTION_PERMISSION,
            None,
            None,
            None,
            None,
            None,
        )
        .await?;
        let hash = request_hash(&json!({"id":id,"input":input}))?;
        let mut tx = self.store.pool().begin().await?;
        if let Some(value) =
            begin_idempotent::<Value>(&mut tx, actor, "operating_subscription_command", key, &hash)
                .await?
        {
            tx.commit().await?;
            return Ok(value);
        }
        let current = sqlx::query("SELECT cadence,currency::text,utc_offset_minutes,delivery_hour,status,version FROM operating_report_subscriptions WHERE id=$1 AND owner_user_id=$2 FOR UPDATE")
            .bind(id).bind(actor).fetch_optional(&mut *tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
        if current.get::<i64, _>("version") != input.expected_version {
            return Err(DomainError::VersionConflict);
        }
        if !matches!(input.action.as_str(), "pause" | "resume") {
            return Err(DomainError::Invalid(
                "subscription action must be pause or resume".into(),
            ));
        }
        let status = if input.action == "pause" {
            "paused"
        } else {
            "active"
        };
        let next_run = next_run_at(
            &current.get::<String, _>("cadence"),
            current.get("utc_offset_minutes"),
            current.get("delivery_hour"),
            Utc::now(),
        );
        let row = sqlx::query("UPDATE operating_report_subscriptions SET status=$2,next_run_at=$3,version=version+1 WHERE id=$1 RETURNING id,cadence,currency::text,utc_offset_minutes,delivery_hour,status,next_run_at,last_run_at,last_snapshot_id,version")
            .bind(id).bind(status).bind(next_run).fetch_one(&mut *tx).await?;
        subscription_event(
            &mut tx,
            id,
            if status == "paused" {
                "paused"
            } else {
                "resumed"
            },
            actor,
            trace_id,
            json!({"nextRunAt":next_run}),
        )
        .await?;
        audit(
            &mut tx,
            trace_id,
            actor,
            "operating_subscription.command",
            "operating_report_subscription",
            &id.to_string(),
            json!({"action":input.action,"status":status,"nextRunAt":next_run}),
        )
        .await?;
        let result = subscription_json(&row);
        finish_idempotent(
            &mut tx,
            actor,
            "operating_subscription_command",
            key,
            &result,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn run_due_operating_subscriptions(&self, limit: i64) -> Result<i64, DomainError> {
        let due = sqlx::query("WITH claimed AS (SELECT id FROM operating_report_subscriptions WHERE status='active' AND next_run_at<=now() ORDER BY next_run_at FOR UPDATE SKIP LOCKED LIMIT $1) UPDATE operating_report_subscriptions s SET next_run_at=now()+interval '15 minutes',last_run_at=now(),version=version+1 FROM claimed WHERE s.id=claimed.id RETURNING s.id,s.owner_user_id,s.cadence,s.currency::text,s.utc_offset_minutes,s.delivery_hour")
            .bind(limit.clamp(1, 50)).fetch_all(self.store.pool()).await?;
        let mut completed = 0_i64;
        for row in due {
            let id: Uuid = row.get("id");
            let actor: Uuid = row.get("owner_user_id");
            let cadence: String = row.get("cadence");
            let offset: i16 = row.get("utc_offset_minutes");
            let hour: i16 = row.get("delivery_hour");
            let trace_id = Uuid::new_v4();
            let local_today = (Utc::now() + Duration::minutes(i64::from(offset))).date_naive();
            let period_start = if cadence == "daily" {
                local_today - Duration::days(1)
            } else {
                local_today
                    - Duration::days(7 + i64::from(local_today.weekday().num_days_from_monday()))
            };
            let input = GenerateOperatingSnapshot {
                cadence: cadence.clone(),
                currency: row.get("currency"),
                period_start,
                utc_offset_minutes: offset,
            };
            let result = self
                .generate_operating_snapshot_once(actor, trace_id, &input)
                .await;
            let mut tx = self.store.pool().begin().await?;
            let next_run = next_run_at(&cadence, offset, hour, Utc::now() + Duration::minutes(1));
            match result {
                Ok(snapshot) => {
                    let snapshot_id = Uuid::parse_str(
                        snapshot["id"]
                            .as_str()
                            .ok_or_else(|| DomainError::Invalid("snapshot id missing".into()))?,
                    )
                    .map_err(|_| DomainError::Invalid("snapshot id invalid".into()))?;
                    sqlx::query("UPDATE operating_report_subscriptions SET next_run_at=$2,last_run_at=now(),last_snapshot_id=$3,version=version+1 WHERE id=$1").bind(id).bind(next_run).bind(snapshot_id).execute(&mut *tx).await?;
                    subscription_event(
                        &mut tx,
                        id,
                        "generated",
                        actor,
                        trace_id,
                        json!({"snapshotId":snapshot_id,"periodStart":period_start}),
                    )
                    .await?;
                    completed += 1;
                }
                Err(error) => {
                    sqlx::query("UPDATE operating_report_subscriptions SET next_run_at=$2,last_run_at=now(),version=version+1 WHERE id=$1").bind(id).bind(next_run).execute(&mut *tx).await?;
                    subscription_event(
                        &mut tx,
                        id,
                        "failed",
                        actor,
                        trace_id,
                        json!({"reason":error.to_string()}),
                    )
                    .await?;
                }
            }
            tx.commit().await?;
        }
        Ok(completed)
    }
}

fn validate_cadence(value: &str) -> Result<(), DomainError> {
    if matches!(value, "daily" | "weekly") {
        Ok(())
    } else {
        Err(DomainError::Invalid(
            "cadence must be daily or weekly".into(),
        ))
    }
}

fn validate_snapshot_input(input: &GenerateOperatingSnapshot) -> Result<(), DomainError> {
    validate_cadence(&input.cadence)?;
    crate::b2::common::validate_currency(&input.currency)?;
    if !(-720..=840).contains(&input.utc_offset_minutes) {
        return Err(DomainError::Invalid(
            "UTC offset is outside the supported range".into(),
        ));
    }
    if input.cadence == "weekly" && input.period_start.weekday() != Weekday::Mon {
        return Err(DomainError::Invalid(
            "weekly period must start on Monday".into(),
        ));
    }
    Ok(())
}

fn validate_subscription(input: &CreateSubscription) -> Result<(), DomainError> {
    validate_cadence(&input.cadence)?;
    crate::b2::common::validate_currency(&input.currency)?;
    if !(-720..=840).contains(&input.utc_offset_minutes) || !(0..=23).contains(&input.delivery_hour)
    {
        return Err(DomainError::Invalid(
            "invalid schedule offset or delivery hour".into(),
        ));
    }
    Ok(())
}

fn next_run_at(cadence: &str, offset: i16, hour: i16, now: DateTime<Utc>) -> DateTime<Utc> {
    let local = now + Duration::minutes(i64::from(offset));
    let mut candidate = local
        .date_naive()
        .and_hms_opt(hour as u32, 0, 0)
        .unwrap_or(local.naive_utc());
    if cadence == "weekly" {
        candidate += Duration::days(i64::from((7 - local.weekday().num_days_from_monday()) % 7));
    }
    if candidate <= local.naive_utc() {
        candidate += Duration::days(if cadence == "daily" { 1 } else { 7 });
    }
    DateTime::<Utc>::from_naive_utc_and_offset(
        candidate - Duration::minutes(i64::from(offset)),
        Utc,
    )
}

fn snapshot_result(row: &sqlx::postgres::PgRow, created: bool, trace_id: Uuid) -> Value {
    json!({"id":row.get::<Uuid,_>("id"),"created":created,"ownerUserId":row.get::<Uuid,_>("generated_by_user_id"),"utcOffsetMinutes":row.get::<Option<i16>,_>("utc_offset_minutes"),"generatedAt":row.get::<DateTime<Utc>,_>("generated_at"),"sourceHash":row.get::<String,_>("source_hash"),"dataQualityStatus":row.get::<String,_>("data_quality_status"),"traceId":trace_id})
}

fn trend_change(current: &Value, previous: &Value) -> Value {
    let fields = [
        "salesOrderAmount",
        "shippedRevenue",
        "purchaseOrderAmount",
        "managementOperatingProfit",
        "slaBreached",
    ];
    Value::Object(
        fields
            .into_iter()
            .map(|field| {
                (
                    field.to_string(),
                    percentage_change(&current[field], &previous[field]),
                )
            })
            .collect(),
    )
}

fn percentage_change(current: &Value, previous: &Value) -> Value {
    let parse = |value: &Value| {
        value
            .as_str()
            .and_then(|text| text.parse::<Decimal>().ok())
            .or_else(|| value.as_i64().map(Decimal::from))
    };
    match (parse(current), parse(previous)) {
        (Some(current), Some(previous)) if previous != Decimal::ZERO => {
            json!(((current - previous) / previous * Decimal::from(100))
                .round_dp(2)
                .to_string())
        }
        _ => Value::Null,
    }
}

fn subscription_json(row: &sqlx::postgres::PgRow) -> Value {
    json!({"id":row.get::<Uuid,_>("id"),"cadence":row.get::<String,_>("cadence"),"currency":row.get::<String,_>("currency"),"utcOffsetMinutes":row.get::<i16,_>("utc_offset_minutes"),"deliveryHour":row.get::<i16,_>("delivery_hour"),"status":row.get::<String,_>("status"),"nextRunAt":row.get::<DateTime<Utc>,_>("next_run_at"),"lastRunAt":row.get::<Option<DateTime<Utc>>,_>("last_run_at"),"lastSnapshotId":row.get::<Option<Uuid>,_>("last_snapshot_id"),"version":row.get::<i64,_>("version")})
}

async fn subscription_event(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    event_type: &str,
    actor: Uuid,
    trace_id: Uuid,
    payload: Value,
) -> Result<(), DomainError> {
    sqlx::query("INSERT INTO operating_report_subscription_events(id,subscription_id,event_type,actor_user_id,trace_id,payload) VALUES($1,$2,$3,$4,$5,$6)").bind(Uuid::new_v4()).bind(id).bind(event_type).bind(actor).bind(trace_id).bind(payload).execute(&mut **tx).await?;
    Ok(())
}
