use super::{model, CrmService};
use crate::{
    b2::common::{
        authorize, begin_idempotent, finish_idempotent, record, request_hash, DomainError,
    },
    master_data::{create_crm_customer, SaveCoreMasterData},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

/// Explicit customer/contact confirmation submitted when closing an opportunity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConvertCustomer {
    pub expected_version: i64,
    pub customer_id: Option<Uuid>,
    pub customer_name: String,
    pub contact_name: String,
    pub contact_details: String,
    pub credit_currency: String,
    pub payment_terms_days: i32,
    pub note: String,
}
impl CrmService {
    /// Convert and close one opportunity atomically, reusing records and preserving other opportunities.
    pub async fn convert_customer(
        &self,
        actor: Uuid,
        trace: Uuid,
        id: Uuid,
        key: &str,
        input: &ConvertCustomer,
    ) -> Result<Value, DomainError> {
        model::text(&input.customer_name, 160, true)?;
        model::text(&input.contact_name, 100, true)?;
        model::text(&input.contact_details, 200, true)?;
        model::text(&input.note, 4000, true)?;
        let previous = self.accessible(actor, id, "crm:manage").await?;
        let scope = self.scope(actor, "crm:manage").await?;
        if input.customer_id.is_none() && previous.customer_id.is_none() {
            authorize(
                &self.store,
                actor,
                "business_master_data:manage",
                None,
                None,
                None,
                None,
                None,
            )
            .await?;
        }
        let mut tx = self.store.pool().begin().await?;
        let hash = request_hash(&(id, input))?;
        if let Some(result) =
            begin_idempotent::<Value>(&mut tx, actor, "crm:convert", key, &hash).await?
        {
            tx.commit().await?;
            return Ok(result);
        }
        let version: i64 =
            sqlx::query_scalar("SELECT version FROM crm_opportunities WHERE id=$1 FOR UPDATE")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        if version != input.expected_version {
            return Err(DomainError::VersionConflict);
        }
        if previous.customer_id.is_some()
            && input.customer_id.is_some()
            && previous.customer_id != input.customer_id
        {
            return Err(DomainError::Invalid(
                "已关联正式客户不可在成交时更换".into(),
            ));
        }
        // Serialize conversions with the same normalized name, including separate opportunities.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!(
                "crm_customer:{}",
                input.customer_name.trim().to_lowercase()
            ))
            .execute(&mut *tx)
            .await?;
        let customer = if let Some(customer) = previous.customer_id.or(input.customer_id) {
            if !scope.scopes.customer_ids.contains(&customer) {
                return Err(DomainError::NotFoundOrForbidden);
            }
            customer
        } else {
            let exists: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM business_customers WHERE lower(btrim(name))=lower($1))").bind(input.customer_name.trim()).fetch_one(&mut *tx).await?;
            if exists {
                return Err(DomainError::Invalid(
                    "同名客户已存在，请选择已有客户；无法找到时请联系管理员核对权限".into(),
                ));
            }
            let master = SaveCoreMasterData {
                resource_type: "customer".into(),
                code: String::new(),
                name: input.customer_name.trim().into(),
                legal_entity_id: None,
                business_unit_id: None,
                parent_business_unit_id: None,
                country_code: None,
                functional_currency: None,
                registration_number: None,
                address: None,
                credit_currency: Some(input.credit_currency.clone()),
                credit_limit_minor: Some(0),
                payment_terms_days: Some(input.payment_terms_days),
                expected_version: None,
            };
            create_crm_customer(&mut tx, actor, trace, &master).await?
        };
        let name: String = sqlx::query_scalar(
            "SELECT name FROM business_customers WHERE id=$1 AND status='active'",
        )
        .bind(customer)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
        // Assignment must not make the existing owner lose access silently.
        if previous.owner_user_id != actor {
            let allowed: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2)").bind(previous.owner_user_id).bind(customer).fetch_one(&mut *tx).await?;
            if !allowed {
                return Err(DomainError::Invalid(
                    "商机负责人尚无该正式客户的访问权限，请先配置客户权限".into(),
                ));
            }
        }
        let account: Uuid=sqlx::query_scalar("INSERT INTO crm_accounts(customer_id,name,owner_user_id) VALUES($1,$2,$3) ON CONFLICT(customer_id) DO UPDATE SET customer_id=excluded.customer_id RETURNING id").bind(customer).bind(&name).bind(actor).fetch_one(&mut *tx).await?;
        let contact: Uuid=sqlx::query_scalar("INSERT INTO crm_contacts(account_id,name,details) VALUES($1,$2,$3) ON CONFLICT(account_id,name,details) DO UPDATE SET name=excluded.name RETURNING id").bind(account).bind(input.contact_name.trim()).bind(input.contact_details.trim()).fetch_one(&mut *tx).await?;
        sqlx::query("UPDATE crm_opportunities SET customer_id=$2,account_id=$3,contact_id=$4,stage='won',loss_reason='',next_action='',next_follow_up=NULL,version=version+1,updated_at=now() WHERE id=$1").bind(id).bind(customer).bind(account).bind(contact).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO crm_followups(id,opportunity_id,author_user_id,note,stage,next_action) VALUES($1,$2,$3,$4,'won','')").bind(Uuid::new_v4()).bind(id).bind(actor).bind(input.note.trim()).execute(&mut *tx).await?;
        let result = json!({"id":id,"customerId":customer,"accountId":account,"contactId":contact,"version":version+1,"traceId":trace});
        record(
            &mut tx,
            trace,
            actor,
            "crm.customer.converted",
            "crm.customer.converted",
            "crm_opportunity",
            id,
            result.clone(),
        )
        .await?;
        finish_idempotent(&mut tx, actor, "crm:convert", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
}
