use super::{model, CrmService, Filters, SaveOpportunity};
use crate::b2::common::{begin_idempotent, finish_idempotent, record, request_hash, DomainError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

/// Minimal prospect record, or a reference to an existing core customer.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveAccount {
    pub name: String,
    pub customer_id: Option<Uuid>,
    pub expected_version: Option<i64>,
}
/// Reusable contact belonging to one customer, independent of opportunities.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveContact {
    pub account_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub details: String,
    pub expected_version: Option<i64>,
}
impl CrmService {
    async fn account_access(
        &self,
        actor: Uuid,
        id: Uuid,
        permission: &str,
    ) -> Result<Option<Uuid>, DomainError> {
        let s = self.scope(actor, permission).await?;
        let customer: Option<Option<Uuid>> = sqlx::query_scalar("SELECT a.customer_id FROM crm_accounts a WHERE a.id=$1 AND ((a.customer_id IS NOT NULL AND a.customer_id=ANY($3)) OR (a.customer_id IS NULL AND (a.owner_user_id=$2 OR EXISTS(SELECT 1 FROM crm_opportunities o WHERE o.deleted_at IS NULL AND o.account_id=a.id AND o.legal_entity_id=ANY($4) AND o.business_unit_id=ANY($5) AND o.customer_id IS NULL))))")
            .bind(id).bind(actor).bind(s.scopes.customer_ids.iter().copied().collect::<Vec<_>>()).bind(s.scopes.legal_entity_ids.iter().copied().collect::<Vec<_>>()).bind(s.scopes.business_unit_ids.iter().copied().collect::<Vec<_>>()).fetch_optional(self.store.pool()).await?;
        customer.ok_or(DomainError::NotFoundOrForbidden)
    }
    /// Customer directory respects core customer grants and existing opportunity access.
    pub async fn accounts(&self, actor: Uuid, filters: &Filters) -> Result<Value, DomainError> {
        let s = self.scope(actor, "crm:read").await?;
        model::text(filters.query.as_deref().unwrap_or(""), 160, false)?;
        if !(0..=100000).contains(&filters.offset) {
            return Err(DomainError::Invalid("无效页码".into()));
        }
        let mut items: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',a.id,'customerId',a.customer_id,'name',COALESCE(c.name,a.name),'version',a.version) FROM crm_accounts a LEFT JOIN business_customers c ON c.id=a.customer_id WHERE ((a.customer_id IS NOT NULL AND a.customer_id=ANY($2)) OR (a.customer_id IS NULL AND (a.owner_user_id=$1 OR EXISTS(SELECT 1 FROM crm_opportunities o WHERE o.deleted_at IS NULL AND o.account_id=a.id AND o.legal_entity_id=ANY($3) AND o.business_unit_id=ANY($4) AND o.customer_id IS NULL)))) AND ($5::text IS NULL OR strpos(lower(COALESCE(c.name,a.name)),lower($5))>0) ORDER BY COALESCE(c.name,a.name),a.id LIMIT 51 OFFSET $6")
            .bind(actor).bind(s.scopes.customer_ids.iter().copied().collect::<Vec<_>>()).bind(s.scopes.legal_entity_ids.iter().copied().collect::<Vec<_>>()).bind(s.scopes.business_unit_ids.iter().copied().collect::<Vec<_>>()).bind(filters.query.as_deref().map(str::trim)).bind(filters.offset).fetch_all(self.store.pool()).await?;
        let has_more = items.len() > 50;
        items.truncate(50);
        Ok(
            json!({"items":items,"hasMore":has_more,"canManage":s.permission_keys.contains("crm:manage")}),
        )
    }
    /// Create or edit a prospect without requiring transaction details or organization binding.
    pub async fn save_account(
        &self,
        actor: Uuid,
        trace: Uuid,
        id: Option<Uuid>,
        key: &str,
        input: &SaveAccount,
    ) -> Result<Value, DomainError> {
        model::text(&input.name, 160, true)?;
        let s = self.scope(actor, "crm:manage").await?;
        if let Some(customer) = input.customer_id {
            if !s.scopes.customer_ids.contains(&customer) {
                return Err(DomainError::NotFoundOrForbidden);
            }
        }
        if let Some(id) = id {
            if self.account_access(actor, id, "crm:manage").await? != input.customer_id {
                return Err(DomainError::Invalid("客户关联不可更换，请新建档案".into()));
            }
        }
        if id.is_some() != input.expected_version.is_some() {
            return Err(DomainError::Invalid("更新需要当前版本".into()));
        }
        let mut tx = self.store.pool().begin().await?;
        let hash = request_hash(&(id, input))?;
        if let Some(result) =
            begin_idempotent::<Value>(&mut tx, actor, "crm:account", key, &hash).await?
        {
            tx.commit().await?;
            return Ok(result);
        }
        let name: String = if let Some(customer) = input.customer_id {
            sqlx::query_scalar(
                "SELECT name FROM business_customers WHERE id=$1 AND status='active'",
            )
            .bind(customer)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(DomainError::NotFoundOrForbidden)?
        } else {
            input.name.trim().to_string()
        };
        let duplicate: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM crm_accounts WHERE ($1::uuid IS NULL OR id<>$1) AND (($2::uuid IS NOT NULL AND customer_id=$2) OR ($2::uuid IS NULL AND customer_id IS NULL AND owner_user_id=$3 AND lower(btrim(name))=lower($4))))").bind(id).bind(input.customer_id).bind(actor).bind(&name).fetch_one(&mut *tx).await?;
        if duplicate {
            return Err(DomainError::Invalid(
                "该客户档案已存在，请搜索后使用".into(),
            ));
        }
        let record_id = id.unwrap_or_else(Uuid::new_v4);
        let version: Option<i64> = if id.is_some() {
            sqlx::query_scalar("UPDATE crm_accounts SET name=$2,version=version+1 WHERE id=$1 AND version=$3 RETURNING version").bind(record_id).bind(&name).bind(input.expected_version).fetch_optional(&mut *tx).await?
        } else {
            sqlx::query_scalar("INSERT INTO crm_accounts(id,customer_id,name,owner_user_id) VALUES($1,$2,$3,$4) RETURNING version").bind(record_id).bind(input.customer_id).bind(&name).bind(actor).fetch_optional(&mut *tx).await?
        };
        let version = version.ok_or(DomainError::VersionConflict)?;
        sqlx::query(
            "UPDATE crm_opportunities SET version=version+1,updated_at=now() WHERE account_id=$1",
        )
        .bind(record_id)
        .execute(&mut *tx)
        .await?;
        let result = json!({"id":record_id,"version":version});
        record(
            &mut tx,
            trace,
            actor,
            "crm.account.saved",
            "crm.account.saved",
            "crm_account",
            record_id,
            json!({"version":version}),
        )
        .await?;
        finish_idempotent(&mut tx, actor, "crm:account", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
    /// Persist a contact once; all linked opportunities read the same current information.
    pub async fn save_contact(
        &self,
        actor: Uuid,
        trace: Uuid,
        id: Option<Uuid>,
        key: &str,
        input: &SaveContact,
    ) -> Result<Value, DomainError> {
        model::text(&input.name, 100, true)?;
        model::text(&input.details, 200, false)?;
        self.account_access(actor, input.account_id, "crm:manage")
            .await?;
        if id.is_some() != input.expected_version.is_some() {
            return Err(DomainError::Invalid("更新需要当前版本".into()));
        }
        let mut tx = self.store.pool().begin().await?;
        let hash = request_hash(&(id, input))?;
        if let Some(result) =
            begin_idempotent::<Value>(&mut tx, actor, "crm:contact", key, &hash).await?
        {
            tx.commit().await?;
            return Ok(result);
        }
        let duplicate: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM crm_contacts WHERE account_id=$1 AND name=$2 AND details=$3 AND ($4::uuid IS NULL OR id<>$4))").bind(input.account_id).bind(input.name.trim()).bind(input.details.trim()).bind(id).fetch_one(&mut *tx).await?;
        if duplicate {
            return Err(DomainError::Invalid("该联系人已存在，请直接选用".into()));
        }
        let record_id = id.unwrap_or_else(Uuid::new_v4);
        let version: Option<i64> = if id.is_some() {
            sqlx::query_scalar("UPDATE crm_contacts SET name=$2,details=$3,version=version+1 WHERE id=$1 AND account_id=$4 AND version=$5 RETURNING version").bind(record_id).bind(input.name.trim()).bind(input.details.trim()).bind(input.account_id).bind(input.expected_version).fetch_optional(&mut *tx).await?
        } else {
            sqlx::query_scalar("INSERT INTO crm_contacts(id,account_id,name,details) VALUES($1,$2,$3,$4) RETURNING version").bind(record_id).bind(input.account_id).bind(input.name.trim()).bind(input.details.trim()).fetch_optional(&mut *tx).await?
        };
        let version = version.ok_or(DomainError::VersionConflict)?;
        sqlx::query(
            "UPDATE crm_opportunities SET version=version+1,updated_at=now() WHERE contact_id=$1",
        )
        .bind(record_id)
        .execute(&mut *tx)
        .await?;
        let result = json!({"id":record_id,"version":version});
        record(
            &mut tx,
            trace,
            actor,
            "crm.contact.saved",
            "crm.contact.saved",
            "crm_contact",
            record_id,
            json!({"version":version}),
        )
        .await?;
        finish_idempotent(&mut tx, actor, "crm:contact", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
    pub(super) async fn resolve_directory(
        &self,
        actor: Uuid,
        input: &SaveOpportunity,
        tx: &mut Transaction<'_, Postgres>,
    ) -> Result<(Uuid, Option<Uuid>), DomainError> {
        let account = if let Some(id) = input.account_id {
            if self.account_access(actor, id, "crm:manage").await? != input.customer_id {
                return Err(DomainError::Invalid("客户关联与商机不一致".into()));
            }
            id
        } else if let Some(customer) = input.customer_id {
            sqlx::query_scalar("INSERT INTO crm_accounts(customer_id,name,owner_user_id) VALUES($1,$2,$3) ON CONFLICT(customer_id) DO UPDATE SET customer_id=excluded.customer_id RETURNING id").bind(customer).bind(input.company_name.trim()).bind(actor).fetch_one(&mut **tx).await?
        } else {
            sqlx::query_scalar("INSERT INTO crm_accounts(name,owner_user_id) VALUES($1,$2) ON CONFLICT(owner_user_id,lower(btrim(name))) WHERE customer_id IS NULL DO UPDATE SET name=crm_accounts.name RETURNING id").bind(input.company_name.trim()).bind(actor).fetch_one(&mut **tx).await?
        };
        let contact = if let Some(id) = input.contact_id {
            let valid: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM crm_contacts WHERE id=$1 AND account_id=$2)",
            )
            .bind(id)
            .bind(account)
            .fetch_one(&mut **tx)
            .await?;
            if !valid {
                return Err(DomainError::Invalid("联系人不属于所选客户".into()));
            }
            Some(id)
        } else if !input.contact_name.trim().is_empty() {
            Some(sqlx::query_scalar("INSERT INTO crm_contacts(account_id,name,details) VALUES($1,$2,$3) ON CONFLICT(account_id,name,details) DO UPDATE SET name=excluded.name RETURNING id").bind(account).bind(input.contact_name.trim()).bind(input.contact_details.trim()).fetch_one(&mut **tx).await?)
        } else {
            None
        };
        Ok((account, contact))
    }
}
