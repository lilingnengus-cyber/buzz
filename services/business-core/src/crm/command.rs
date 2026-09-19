//! Exact CRM commands and locked previews shared by preparation and execution.
use super::{model, AddFollowup, CrmService, Opportunity, SaveOpportunity};
use crate::{
    b2::common::{request_hash, DomainError},
    store::PgStore,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

/// A fixed CRM operation. Updating and following up require a current version.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum CrmCommand {
    /// Create a presales opportunity; does not create a sales order.
    Create { command: SaveOpportunity },
    /// Replace editable opportunity fields, preserving its legal entity and unit.
    Update {
        #[serde(rename = "opportunityId")]
        opportunity_id: Uuid,
        command: SaveOpportunity,
    },
    /// Append an immutable note and update the next action and stage atomically.
    Followup {
        #[serde(rename = "opportunityId")]
        opportunity_id: Uuid,
        command: AddFollowup,
    },
}
impl CrmCommand {
    /// Fixed approval intent family for this operation.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Create { .. } => "crm_creation_intent",
            Self::Update { .. } => "crm_update_intent",
            Self::Followup { .. } => "crm_followup_intent",
        }
    }
}
pub(super) struct Guard<'a> {
    pub snapshot: &'a Value,
    pub request: Option<Uuid>,
}

impl CrmService {
    /// Compute a read-only preview from locked current records and current authority.
    pub async fn command_preview(
        &self,
        actor: Uuid,
        command: &CrmCommand,
    ) -> Result<Value, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        let snapshot = self.preview_on(&mut tx, actor, command).await?;
        tx.rollback().await?;
        Ok(snapshot)
    }

    pub(super) async fn preview_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        command: &CrmCommand,
    ) -> Result<Value, DomainError> {
        let id = match command {
            CrmCommand::Create { command } => {
                command.validate()?;
                if command.expected_version.is_some() {
                    return Err(DomainError::VersionConflict);
                }
                None
            }
            CrmCommand::Update {
                opportunity_id,
                command,
            } => {
                command.validate()?;
                if command.expected_version.is_none_or(|v| v < 1) {
                    return Err(DomainError::VersionConflict);
                }
                Some(*opportunity_id)
            }
            CrmCommand::Followup {
                opportunity_id,
                command,
            } => {
                model::text(&command.note, 4000, true)?;
                model::text(&command.next_action, 500, false)?;
                model::stage(&command.stage)?;
                if command.expected_version < 1 {
                    return Err(DomainError::VersionConflict);
                }
                Some(*opportunity_id)
            }
        };
        let current = if let Some(id) = id {
            Some(
                sqlx::query_as::<_, Opportunity>(
                    "SELECT * FROM crm_opportunities WHERE id=$1 FOR UPDATE",
                )
                .bind(id)
                .fetch_optional(&mut **tx)
                .await?
                .ok_or(DomainError::NotFoundOrForbidden)?,
            )
        } else {
            None
        };
        let (legal, unit, customer) = match command {
            CrmCommand::Create { command } | CrmCommand::Update { command, .. } => {
                if let Some(old) = &current {
                    if old.legal_entity_id != command.legal_entity_id
                        || old.business_unit_id != command.business_unit_id
                    {
                        return Err(DomainError::Invalid("商机所属主体不可更改".into()));
                    }
                    if Some(old.version) != command.expected_version {
                        return Err(DomainError::VersionConflict);
                    }
                }
                (
                    command.legal_entity_id,
                    command.business_unit_id,
                    command.customer_id,
                )
            }
            CrmCommand::Followup { command, .. } => {
                let old = current.as_ref().ok_or(DomainError::NotFoundOrForbidden)?;
                if old.version != command.expected_version {
                    return Err(DomainError::VersionConflict);
                }
                (old.legal_entity_id, old.business_unit_id, old.customer_id)
            }
        };
        // Parent status/names/versions are part of the approval, and locked until
        // execution commits. A disabled or moved customer cannot slip past preview.
        let entity: Value = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'code',code,'name',name,'status',status,'version',version,'updatedAt',updated_at) FROM business_legal_entities e WHERE id=$1 AND status='active' FOR SHARE")
            .bind(legal).fetch_optional(&mut **tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
        let business_unit: Value = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'code',code,'name',name,'status',status,'version',version,'updatedAt',updated_at,'legalEntityId',legal_entity_id) FROM business_units u WHERE id=$1 AND legal_entity_id=$2 AND status='active' FOR SHARE")
            .bind(unit).bind(legal).fetch_optional(&mut **tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
        let party: Option<Value> = if let Some(id) = customer {
            Some(sqlx::query_scalar("SELECT jsonb_build_object('id',id,'code',code,'name',name,'status',status,'version',version,'updatedAt',updated_at,'legalEntityId',legal_entity_id,'businessUnitId',business_unit_id) FROM business_customers c WHERE id=$1 AND legal_entity_id=$2 AND business_unit_id=$3 AND status='active' FOR SHARE")
                .bind(id).bind(legal).bind(unit).fetch_optional(&mut **tx).await?.ok_or(DomainError::NotFoundOrForbidden)?)
        } else {
            None
        };
        sqlx::query(
            "SELECT revision FROM business_authorization_revision WHERE singleton FOR SHARE",
        )
        .fetch_one(&mut **tx)
        .await?;
        let scope = PgStore::snapshot_on(&mut *tx, actor)
            .await
            .map_err(|_| DomainError::NotFoundOrForbidden)?;
        if !scope.permission_keys.contains("crm:manage")
            || !scope.scopes.legal_entity_ids.contains(&legal)
            || !scope.scopes.business_unit_ids.contains(&unit)
            || customer.is_some_and(|v| !scope.scopes.customer_ids.contains(&v))
            || current
                .as_ref()
                .and_then(|v| v.customer_id)
                .is_some_and(|v| !scope.scopes.customer_ids.contains(&v))
        {
            return Err(DomainError::NotFoundOrForbidden);
        }
        Ok(
            json!({"documentType":command.kind(),"legalEntityId":legal,"businessUnitId":unit,
            "customerId":customer,"opportunityId":id,"current":current,"command":command,
            "legalEntity":entity,"businessUnit":business_unit,"customer":party}),
        )
    }

    /// Execute only the exact command and current state represented by the supplied preview.
    pub async fn execute_guarded(
        &self,
        actor: Uuid,
        trace: Uuid,
        key: &str,
        command: &CrmCommand,
        snapshot: &Value,
    ) -> Result<Value, DomainError> {
        self.execute_inner(
            (actor, trace),
            key,
            command,
            Guard {
                snapshot,
                request: None,
            },
        )
        .await
    }
    pub(crate) async fn execute_approved(
        &self,
        actor: Uuid,
        trace: Uuid,
        command: &CrmCommand,
        snapshot: &Value,
        request: Uuid,
    ) -> Result<Value, DomainError> {
        self.execute_inner(
            (actor, trace),
            &format!("agent-crm:{request}"),
            command,
            Guard {
                snapshot,
                request: Some(request),
            },
        )
        .await
    }
    async fn execute_inner(
        &self,
        context: (Uuid, Uuid),
        key: &str,
        command: &CrmCommand,
        guard: Guard<'_>,
    ) -> Result<Value, DomainError> {
        match command {
            CrmCommand::Create { command } => {
                self.save_inner(context, None, key, command, Some(guard))
                    .await
            }
            CrmCommand::Update {
                opportunity_id,
                command,
            } => {
                self.save_inner(context, Some(*opportunity_id), key, command, Some(guard))
                    .await
            }
            CrmCommand::Followup {
                opportunity_id,
                command,
            } => {
                self.followup_inner(context, *opportunity_id, key, command, Some(guard))
                    .await
            }
        }
    }
}

pub(super) async fn finish_approval(
    tx: &mut Transaction<'_, Postgres>,
    guard: Option<&Guard<'_>>,
) -> Result<(), DomainError> {
    if let Some(Guard {
        snapshot,
        request: Some(request),
    }) = guard
    {
        let changed = sqlx::query("UPDATE business_document_approval_requests r SET status='executed',executed_at=now(),version=version+1 WHERE id=$1 AND status='executing' AND preview_hash=$2 AND document_type=$3 AND EXISTS(SELECT 1 FROM business_agent_crm_intents i WHERE i.id=r.document_id AND i.kind=r.document_type AND i.expires_at>clock_timestamp() AND i.snapshot=$4)")
            .bind(request).bind(request_hash(snapshot)?).bind(snapshot["documentType"].as_str().ok_or(DomainError::StalePreview)?).bind(snapshot)
            .execute(&mut **tx).await?.rows_affected();
        if changed != 1 {
            return Err(DomainError::StalePreview);
        }
    }
    Ok(())
}
