mod command;
mod status;
mod write_authority;
use crate::{
    b2::common::{begin_idempotent, finish_idempotent, record, request_hash, DomainError},
    model::AuthorizationSnapshot,
    operating_units::{has_active_descendants, validate_parent},
    store::{outbox, PgStore},
};
use chrono::Utc;
pub use command::CoreMasterCommand;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{AssertSqlSafe, Row};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreMasterType {
    LegalEntity,
    BusinessUnit,
    Customer,
    Supplier,
    Warehouse,
}

impl CoreMasterType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LegalEntity => "legal_entity",
            Self::BusinessUnit => "business_unit",
            Self::Customer => "customer",
            Self::Supplier => "supplier",
            Self::Warehouse => "warehouse",
        }
    }
}

impl FromStr for CoreMasterType {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "legal_entity" => Ok(Self::LegalEntity),
            "business_unit" => Ok(Self::BusinessUnit),
            "customer" => Ok(Self::Customer),
            "supplier" => Ok(Self::Supplier),
            "warehouse" => Ok(Self::Warehouse),
            _ => Err(DomainError::Invalid(
                "unsupported core master data type".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveCoreMasterData {
    pub resource_type: String,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub legal_entity_id: Option<Uuid>,
    #[serde(default)]
    pub business_unit_id: Option<Uuid>,
    #[serde(default)]
    pub parent_business_unit_id: Option<Uuid>,
    #[serde(default)]
    pub country_code: Option<String>,
    #[serde(default)]
    pub functional_currency: Option<String>,
    #[serde(default)]
    pub registration_number: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub credit_currency: Option<String>,
    #[serde(default)]
    pub credit_limit_minor: Option<i64>,
    #[serde(default)]
    pub payment_terms_days: Option<i32>,
    #[serde(default)]
    pub expected_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeCoreMasterStatus {
    pub status: String,
    pub expected_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreMasterCommandResult {
    pub id: Uuid,
    pub resource_type: String,
    pub code: String,
    pub status: String,
    pub version: i64,
    pub trace_id: Uuid,
    pub idempotent_replay: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CoreMasterRecord {
    pub resource_type: String,
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub status: String,
    pub legal_entity_id: Option<Uuid>,
    pub legal_entity_code: Option<String>,
    pub legal_entity_name: Option<String>,
    pub business_unit_id: Option<Uuid>,
    pub business_unit_code: Option<String>,
    pub business_unit_name: Option<String>,
    pub country_code: Option<String>,
    pub functional_currency: Option<String>,
    pub registration_number: Option<String>,
    pub address: Option<String>,
    pub credit_currency: Option<String>,
    pub credit_limit_minor: Option<i64>,
    pub payment_terms_days: Option<i32>,
    pub version: i64,
    pub updated_at: chrono::DateTime<Utc>,
    pub parent_business_unit_id: Option<Uuid>,
    pub business_unit_path: Option<Vec<String>>,
    pub business_unit_depth: Option<i32>,
    pub descendant_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreMasterList {
    pub items: Vec<CoreMasterRecord>,
    pub can_manage: bool,
    pub data_as_of: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpactItem {
    pub code: String,
    pub label: String,
    pub count: i64,
    pub blocking: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisableImpact {
    pub resource_type: String,
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub status: String,
    pub version: i64,
    pub can_disable: bool,
    pub impacts: Vec<ImpactItem>,
    pub checked_at: chrono::DateTime<Utc>,
}

#[derive(Clone)]
pub struct CoreMasterDataService {
    store: PgStore,
}

impl CoreMasterDataService {
    pub fn new(store: PgStore) -> Self {
        Self { store }
    }

    async fn snapshot(
        &self,
        actor: Uuid,
        permission: &str,
    ) -> Result<AuthorizationSnapshot, DomainError> {
        let snapshot = self
            .store
            .snapshot(actor)
            .await
            .map_err(|_| DomainError::NotFoundOrForbidden)?;
        if snapshot.permission_keys.contains(permission) {
            Ok(snapshot)
        } else {
            Err(DomainError::NotFoundOrForbidden)
        }
    }

    pub async fn list(
        &self,
        actor: Uuid,
        resource_type: Option<CoreMasterType>,
        limit: i64,
    ) -> Result<CoreMasterList, DomainError> {
        let snapshot = self.snapshot(actor, "business_master_data:read").await?;
        let entities = snapshot
            .scopes
            .legal_entity_ids
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let units = snapshot
            .scopes
            .business_unit_ids
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let warehouses = snapshot
            .scopes
            .warehouse_ids
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let customers = snapshot
            .scopes
            .customer_ids
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let suppliers = snapshot
            .scopes
            .supplier_ids
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let items=sqlx::query_as::<_,CoreMasterRecord>("SELECT resource_type,id,code,name,status,legal_entity_id,legal_entity_code,legal_entity_name,business_unit_id,business_unit_code,business_unit_name,country_code,functional_currency,registration_number,address,credit_currency,credit_limit_minor,payment_terms_days,version,updated_at,parent_business_unit_id,business_unit_path,business_unit_depth,descendant_count FROM core_master_data_maintenance WHERE ($1::text IS NULL OR resource_type=$1) AND ((resource_type='legal_entity' AND id=ANY($2)) OR (resource_type='business_unit' AND id=ANY($3)) OR (resource_type NOT IN ('legal_entity','business_unit') AND legal_entity_id=ANY($2) AND business_unit_id=ANY($3))) AND (resource_type<>'warehouse' OR id=ANY($4)) AND (resource_type<>'customer' OR id=ANY($5)) AND (resource_type<>'supplier' OR id=ANY($6)) ORDER BY CASE resource_type WHEN 'legal_entity' THEN 0 WHEN 'business_unit' THEN 1 WHEN 'customer' THEN 2 WHEN 'supplier' THEN 3 ELSE 4 END,code LIMIT $7")
            .bind(resource_type.map(CoreMasterType::as_str)).bind(entities).bind(units).bind(warehouses).bind(customers).bind(suppliers).bind(limit.clamp(1,1000)).fetch_all(self.store.pool()).await?;
        Ok(CoreMasterList {
            items,
            can_manage: snapshot
                .permission_keys
                .contains("business_master_data:manage"),
            data_as_of: Utc::now(),
        })
    }

    pub async fn save(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        id: Option<Uuid>,
        key: &str,
        input: &SaveCoreMasterData,
    ) -> Result<CoreMasterCommandResult, DomainError> {
        self.save_inner((actor, trace_id), id, key, input, None)
            .await
    }

    async fn save_inner(
        &self,
        context: (Uuid, Uuid),
        id: Option<Uuid>,
        key: &str,
        input: &SaveCoreMasterData,
        guard: Option<&serde_json::Value>,
    ) -> Result<CoreMasterCommandResult, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        let result = self
            .save_on(&mut tx, context, id, key, input, guard)
            .await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Save on a caller-owned transaction so approval and business changes commit together.
    pub(crate) async fn save_on(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        context: (Uuid, Uuid),
        id: Option<Uuid>,
        key: &str,
        input: &SaveCoreMasterData,
        guard: Option<&serde_json::Value>,
    ) -> Result<CoreMasterCommandResult, DomainError> {
        let (actor, trace_id) = context;
        let kind = CoreMasterType::from_str(&input.resource_type)?;
        validate(input, kind, id.is_some())?;
        let mut snapshot =
            crate::master_write_authority::read(tx, actor, "business_master_data:manage").await?;
        let hash = match guard {
            Some(snapshot) => request_hash(&("guarded-master-save-v1", id, input, snapshot))?,
            None => request_hash(&(id, input))?,
        };
        if let Some(mut replay) = begin_idempotent::<CoreMasterCommandResult>(
            tx,
            actor,
            "core_master_data:save",
            key,
            &hash,
        )
        .await?
        {
            self.existing_write_authority(tx, actor, kind, replay.id)
                .await?;
            replay.idempotent_replay = true;
            return Ok(replay);
        }
        let target_id = id.unwrap_or_else(Uuid::new_v4);
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("{}:{target_id}", kind.as_str()))
            .execute(&mut **tx)
            .await?;
        if let Some(expected) = guard {
            let command = match id {
                Some(document_id) => CoreMasterCommand::Update {
                    document_id,
                    command: input.clone(),
                },
                None => CoreMasterCommand::Create {
                    command: input.clone(),
                },
            };
            if self.preview_on(tx, actor, &command).await? != *expected {
                return Err(DomainError::StalePreview);
            }
        }
        if kind == CoreMasterType::BusinessUnit {
            if input
                .parent_business_unit_id
                .is_some_and(|parent| !snapshot.scopes.business_unit_ids.contains(&parent))
            {
                return Err(DomainError::NotFoundOrForbidden);
            }
            validate_parent(&mut tx, target_id, input.parent_business_unit_id).await?;
        }
        if let Some(existing_id) = id {
            snapshot = self
                .existing_write_authority(tx, actor, kind, existing_id)
                .await?;
            let current=sqlx::query("SELECT code,status,version,legal_entity_id,business_unit_id FROM core_master_data_maintenance WHERE resource_type=$1 AND id=$2").bind(kind.as_str()).bind(existing_id).fetch_optional(&mut **tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
            if current.get::<i64, _>("version") != input.expected_version.unwrap_or(0) {
                return Err(DomainError::VersionConflict);
            }
            self.ensure_scope(
                &snapshot,
                kind,
                current.get("legal_entity_id"),
                current.get("business_unit_id"),
                existing_id,
            )?;
            update_record(tx, kind, existing_id, input, actor, trace_id).await?;
        } else {
            if input.expected_version.is_some() {
                return Err(DomainError::VersionConflict);
            }
            if input
                .legal_entity_id
                .is_some_and(|value| !snapshot.scopes.legal_entity_ids.contains(&value))
                || input
                    .business_unit_id
                    .is_some_and(|value| !snapshot.scopes.business_unit_ids.contains(&value))
            {
                return Err(DomainError::NotFoundOrForbidden);
            }
            ensure_parents(tx, kind, input).await?;
            insert_record(tx, kind, target_id, input).await?;
            snapshot = crate::master_write_authority::snapshot(
                tx,
                actor,
                "business_master_data:manage",
                true,
            )
            .await?;
            if input
                .legal_entity_id
                .is_some_and(|v| !snapshot.scopes.legal_entity_ids.contains(&v))
                || input
                    .business_unit_id
                    .is_some_and(|v| !snapshot.scopes.business_unit_ids.contains(&v))
            {
                return Err(DomainError::NotFoundOrForbidden);
            }
            grant_creator_scope(tx, kind, target_id, actor).await?;
        }
        let row=sqlx::query("SELECT code,status,version,legal_entity_id,business_unit_id FROM core_master_data_maintenance WHERE resource_type=$1 AND id=$2").bind(kind.as_str()).bind(target_id).fetch_one(&mut **tx).await?;
        if id.is_some() {
            self.ensure_scope(
                &snapshot,
                kind,
                row.get("legal_entity_id"),
                row.get("business_unit_id"),
                target_id,
            )?;
        }
        let version: i64 = row.get("version");
        record(tx,trace_id,actor,"CORE_MASTER_DATA_SAVED","core_master_data_saved",kind.as_str(),target_id,json!({"resourceType":kind.as_str(),"code":row.get::<String,_>("code"),"version":version,"mode":if id.is_some(){"update"}else{"create"}})).await?;
        let result = CoreMasterCommandResult {
            id: target_id,
            resource_type: kind.as_str().into(),
            code: row.get("code"),
            status: row.get("status"),
            version,
            trace_id,
            idempotent_replay: false,
        };
        finish_idempotent(tx, actor, "core_master_data:save", key, &result).await?;
        Ok(result)
    }

    pub async fn impact(
        &self,
        actor: Uuid,
        kind: CoreMasterType,
        id: Uuid,
    ) -> Result<DisableImpact, DomainError> {
        let snapshot = self.snapshot(actor, "business_master_data:read").await?;
        let row=sqlx::query("SELECT code,name,status,version,legal_entity_id,business_unit_id FROM core_master_data_maintenance WHERE resource_type=$1 AND id=$2").bind(kind.as_str()).bind(id).fetch_optional(self.store.pool()).await?.ok_or(DomainError::NotFoundOrForbidden)?;
        self.ensure_scope(
            &snapshot,
            kind,
            row.get("legal_entity_id"),
            row.get("business_unit_id"),
            id,
        )?;
        let impacts = load_impacts(self.store.pool(), kind, id).await?;
        Ok(DisableImpact {
            resource_type: kind.as_str().into(),
            id,
            code: row.get("code"),
            name: row.get("name"),
            status: row.get("status"),
            version: row.get("version"),
            can_disable: !impacts.iter().any(|item| item.blocking && item.count > 0),
            impacts,
            checked_at: Utc::now(),
        })
    }

    pub async fn change_status(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        kind: CoreMasterType,
        id: Uuid,
        key: &str,
        input: &ChangeCoreMasterStatus,
    ) -> Result<CoreMasterCommandResult, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        let result = self
            .change_status_on(&mut tx, (actor, trace_id), (kind, id), key, input, None)
            .await?;
        tx.commit().await?;
        Ok(result)
    }

    fn ensure_scope(
        &self,
        s: &AuthorizationSnapshot,
        k: CoreMasterType,
        legal: Option<Uuid>,
        unit: Option<Uuid>,
        id: Uuid,
    ) -> Result<(), DomainError> {
        let legal_ok = match k {
            CoreMasterType::LegalEntity => s.scopes.legal_entity_ids.contains(&id),
            CoreMasterType::BusinessUnit => true,
            _ => legal.is_some_and(|v| s.scopes.legal_entity_ids.contains(&v)),
        };
        let unit_ok = match k {
            CoreMasterType::BusinessUnit => s.scopes.business_unit_ids.contains(&id),
            _ => unit.is_none_or(|v| s.scopes.business_unit_ids.contains(&v)),
        };
        let ok = legal_ok
            && unit_ok
            && match k {
                CoreMasterType::Warehouse => s.scopes.warehouse_ids.contains(&id),
                CoreMasterType::Customer => s.scopes.customer_ids.contains(&id),
                CoreMasterType::Supplier => s.scopes.supplier_ids.contains(&id),
                _ => true,
            };
        if ok {
            Ok(())
        } else {
            Err(DomainError::NotFoundOrForbidden)
        }
    }
}

fn validate(i: &SaveCoreMasterData, k: CoreMasterType, updating: bool) -> Result<(), DomainError> {
    let code_ok = (2..=32).contains(&i.code.len())
        && i.code
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b"_-".contains(&b));
    if !code_ok || i.name.trim().is_empty() || i.name.chars().count() > 200 {
        return Err(DomainError::Invalid("code or name is invalid".into()));
    }
    if updating && i.expected_version.is_none() {
        return Err(DomainError::Invalid("expectedVersion is required".into()));
    }
    if i.payment_terms_days
        .is_some_and(|v| !(0..=3650).contains(&v))
        || i.credit_limit_minor.is_some_and(|v| v < 0)
    {
        return Err(DomainError::Invalid(
            "terms or credit limit is invalid".into(),
        ));
    }
    match k {
        CoreMasterType::LegalEntity => {
            currency(i.functional_currency.as_deref())?;
            if i.country_code
                .as_deref()
                .is_none_or(|v| v.len() != 2 || !v.bytes().all(|b| b.is_ascii_uppercase()))
            {
                return Err(DomainError::Invalid("countryCode is invalid".into()));
            }
        }
        CoreMasterType::BusinessUnit => {
            if i.legal_entity_id.is_some() {
                return Err(DomainError::Invalid(
                    "legalEntityId is not valid for an operating unit".into(),
                ));
            }
            if !updating && i.parent_business_unit_id.is_none() {
                return Err(DomainError::Invalid(
                    "parentBusinessUnitId is required".into(),
                ));
            }
        }
        CoreMasterType::Customer => {
            if i.legal_entity_id.is_none() || i.business_unit_id.is_none() {
                return Err(DomainError::Invalid(
                    "legalEntityId and businessUnitId are required".into(),
                ));
            }
            currency(i.credit_currency.as_deref())?;
        }
        CoreMasterType::Supplier | CoreMasterType::Warehouse => {
            if i.legal_entity_id.is_none() || i.business_unit_id.is_none() {
                return Err(DomainError::Invalid(
                    "legalEntityId and businessUnitId are required".into(),
                ));
            }
        }
    }
    Ok(())
}
fn currency(v: Option<&str>) -> Result<(), DomainError> {
    if v.is_some_and(|v| v.len() == 3 && v.bytes().all(|b| b.is_ascii_uppercase())) {
        Ok(())
    } else {
        Err(DomainError::Invalid("currency is invalid".into()))
    }
}

async fn ensure_parents(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    k: CoreMasterType,
    i: &SaveCoreMasterData,
) -> Result<(), DomainError> {
    if matches!(
        k,
        CoreMasterType::Customer | CoreMasterType::Supplier | CoreMasterType::Warehouse
    ) {
        let legal = i
            .legal_entity_id
            .ok_or_else(|| DomainError::Invalid("legalEntityId is required".into()))?;
        let unit = i.business_unit_id;
        let ok:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM business_legal_entities e WHERE e.id=$1 AND e.status='active' AND ($2::uuid IS NULL OR EXISTS(SELECT 1 FROM business_units u WHERE u.id=$2 AND u.legal_entity_id=e.id AND u.status='active')))").bind(legal).bind(unit).fetch_one(&mut **tx).await?;
        if !ok {
            return Err(DomainError::NotFoundOrForbidden);
        }
    }
    Ok(())
}

async fn insert_record(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    k: CoreMasterType,
    id: Uuid,
    i: &SaveCoreMasterData,
) -> Result<(), DomainError> {
    match k {
        CoreMasterType::LegalEntity => {
            sqlx::query("INSERT INTO business_legal_entities(id,code,name,country_code,functional_currency,registration_number) VALUES($1,$2,$3,$4,$5,$6)").bind(id).bind(&i.code).bind(i.name.trim()).bind(i.country_code.as_deref()).bind(i.functional_currency.as_deref()).bind(&i.registration_number).execute(&mut **tx).await?;
        }
        CoreMasterType::BusinessUnit => {
            sqlx::query("INSERT INTO business_units(id,legal_entity_id,parent_business_unit_id,code,name) SELECT $1,parent.legal_entity_id,parent.id,$3,$4 FROM business_units parent WHERE parent.id=$2")
            .bind(id)
            .bind(i.parent_business_unit_id)
            .bind(&i.code)
            .bind(i.name.trim())
            .execute(&mut **tx)
            .await?;
        }
        CoreMasterType::Customer => {
            sqlx::query("INSERT INTO business_customers(id,legal_entity_id,business_unit_id,code,name,credit_currency,credit_limit_minor,payment_terms_days) VALUES($1,$2,$3,$4,$5,$6,$7,$8)").bind(id).bind(i.legal_entity_id).bind(i.business_unit_id).bind(&i.code).bind(i.name.trim()).bind(i.credit_currency.as_deref()).bind(i.credit_limit_minor.unwrap_or(0)).bind(i.payment_terms_days.unwrap_or(30)).execute(&mut **tx).await?;
        }
        CoreMasterType::Supplier => {
            sqlx::query("INSERT INTO business_suppliers(id,legal_entity_id,business_unit_id,code,name,payment_terms_days) VALUES($1,$2,$3,$4,$5,$6)").bind(id).bind(i.legal_entity_id).bind(i.business_unit_id).bind(&i.code).bind(i.name.trim()).bind(i.payment_terms_days.unwrap_or(30)).execute(&mut **tx).await?;
        }
        CoreMasterType::Warehouse => {
            sqlx::query("INSERT INTO business_warehouses(id,legal_entity_id,business_unit_id,code,name,address) VALUES($1,$2,$3,$4,$5,$6)").bind(id).bind(i.legal_entity_id).bind(i.business_unit_id).bind(&i.code).bind(i.name.trim()).bind(&i.address).execute(&mut **tx).await?;
        }
    }
    Ok(())
}

async fn update_record(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    k: CoreMasterType,
    id: Uuid,
    i: &SaveCoreMasterData,
    actor: Uuid,
    trace: Uuid,
) -> Result<(), DomainError> {
    let _ = (actor, trace);
    match k {
        CoreMasterType::LegalEntity => {
            sqlx::query("UPDATE business_legal_entities SET name=$2,country_code=$3,functional_currency=$4,registration_number=$5,version=version+1,updated_at=now() WHERE id=$1").bind(id).bind(i.name.trim()).bind(i.country_code.as_deref()).bind(i.functional_currency.as_deref()).bind(&i.registration_number).execute(&mut **tx).await?;
        }
        CoreMasterType::BusinessUnit => {
            let old_parent: Option<Uuid> = sqlx::query_scalar(
                "SELECT parent_business_unit_id FROM business_units WHERE id=$1",
            )
            .bind(id)
            .fetch_one(&mut **tx)
            .await?;
            sqlx::query("UPDATE business_units SET name=$2,parent_business_unit_id=$3,version=version+1,updated_at=now() WHERE id=$1")
            .bind(id)
            .bind(i.name.trim())
            .bind(i.parent_business_unit_id)
            .execute(&mut **tx)
            .await?;
            if old_parent != i.parent_business_unit_id {
                sqlx::query("UPDATE business_authorization_revision SET revision=revision+1,updated_at=now() WHERE singleton")
                    .execute(&mut **tx)
                    .await?;
                outbox(
                    tx,
                    "business.authorization.changed",
                    "operating_unit",
                    &id.to_string(),
                    json!({"reason":"operating_unit_moved","oldParentBusinessUnitId":old_parent,"newParentBusinessUnitId":i.parent_business_unit_id,"actorUserId":actor,"traceId":trace}),
                )
                .await?;
            }
        }
        CoreMasterType::Customer => {
            sqlx::query("UPDATE business_customers SET name=$2,credit_currency=$3,credit_limit_minor=$4,payment_terms_days=$5,version=version+1,updated_at=now() WHERE id=$1").bind(id).bind(i.name.trim()).bind(i.credit_currency.as_deref()).bind(i.credit_limit_minor.unwrap_or(0)).bind(i.payment_terms_days.unwrap_or(30)).execute(&mut **tx).await?;
        }
        CoreMasterType::Supplier => {
            sqlx::query("UPDATE business_suppliers SET name=$2,payment_terms_days=$3,version=version+1,updated_at=now() WHERE id=$1").bind(id).bind(i.name.trim()).bind(i.payment_terms_days.unwrap_or(30)).execute(&mut **tx).await?;
        }
        CoreMasterType::Warehouse => {
            sqlx::query("UPDATE business_warehouses SET name=$2,address=$3,version=version+1,updated_at=now() WHERE id=$1").bind(id).bind(i.name.trim()).bind(&i.address).execute(&mut **tx).await?;
        }
    }
    Ok(())
}

async fn update_status(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    k: CoreMasterType,
    id: Uuid,
    status: &str,
) -> Result<(), DomainError> {
    let table = match k {
        CoreMasterType::LegalEntity => "business_legal_entities",
        CoreMasterType::BusinessUnit => "business_units",
        CoreMasterType::Customer => "business_customers",
        CoreMasterType::Supplier => "business_suppliers",
        CoreMasterType::Warehouse => "business_warehouses",
    };
    let sql =
        format!("UPDATE {table} SET status=$2,version=version+1,updated_at=now() WHERE id=$1");
    sqlx::query(AssertSqlSafe(sql))
        .bind(id)
        .bind(status)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn grant_creator_scope(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    k: CoreMasterType,
    id: Uuid,
    actor: Uuid,
) -> Result<(), DomainError> {
    // Parent scopes were checked under the revision lock. Only grant the newly
    // created object; never try to recreate or lock an existing parent grant.
    let table = match k {
        CoreMasterType::LegalEntity => "business_legal_entity_scopes",
        CoreMasterType::BusinessUnit => "business_unit_scopes",
        CoreMasterType::Warehouse => "business_warehouse_scopes",
        CoreMasterType::Customer => "business_customer_scopes",
        CoreMasterType::Supplier => "business_supplier_scopes",
    };
    let column = format!("{}_id", k.as_str());
    let sql =
        format!("INSERT INTO {table}(enterprise_user_id,{column},granted_by) VALUES($1,$2,$1)");
    sqlx::query(AssertSqlSafe(sql))
        .bind(actor)
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn load_impacts(
    pool: &sqlx::PgPool,
    kind: CoreMasterType,
    id: Uuid,
) -> Result<Vec<ImpactItem>, DomainError> {
    let mut connection = pool.acquire().await?;
    load_impacts_on(&mut connection, kind, id).await
}

async fn load_impacts_on(
    connection: &mut sqlx::PgConnection,
    k: CoreMasterType,
    id: Uuid,
) -> Result<Vec<ImpactItem>, DomainError> {
    let queries:&[(&str,&str,&str,bool)]=match k{
CoreMasterType::LegalEntity=>&[("active_units","启用中的经营主体","SELECT count(*) FROM business_units WHERE legal_entity_id=$1 AND status='active'",true),("open_sales","未完成销售订单","SELECT count(*) FROM sales_orders WHERE legal_entity_id=$1 AND lifecycle_status IN ('draft','confirmed')",true),("open_purchase","未完成采购订单","SELECT count(*) FROM purchase_orders WHERE legal_entity_id=$1 AND lifecycle_status IN ('draft','confirmed')",true),("stock","存在库存的商品仓位","SELECT count(*) FROM inventory_balances WHERE legal_entity_id=$1 AND (on_hand_quantity<>0 OR reserved_quantity<>0 OR quarantined_quantity<>0)",true)],
CoreMasterType::BusinessUnit=>&[("active_warehouses","启用中的仓库","SELECT count(*) FROM business_warehouses WHERE business_unit_id=$1 AND status='active'",true),("active_partners","启用中的客户或供应商","SELECT (SELECT count(*) FROM business_customers WHERE business_unit_id=$1 AND status='active')+(SELECT count(*) FROM business_suppliers WHERE business_unit_id=$1 AND status='active')",true),("open_orders","未完成销售或采购订单","SELECT (SELECT count(*) FROM sales_orders WHERE business_unit_id=$1 AND lifecycle_status IN ('draft','confirmed'))+(SELECT count(*) FROM purchase_orders WHERE business_unit_id=$1 AND lifecycle_status IN ('draft','confirmed'))",true)],
CoreMasterType::Customer=>&[("open_orders","未完成销售订单","SELECT count(*) FROM sales_orders WHERE customer_id=$1 AND lifecycle_status IN ('draft','confirmed')",true),("open_receivables","未结经营应收","SELECT count(*) FROM trade_receivables WHERE customer_id=$1 AND status IN ('open','partially_settled')",false)],
CoreMasterType::Supplier=>&[("open_orders","未完成采购订单","SELECT count(*) FROM purchase_orders WHERE supplier_id=$1 AND lifecycle_status IN ('draft','confirmed')",true),("open_payables","未结经营应付","SELECT count(*) FROM trade_payables WHERE supplier_id=$1 AND status IN ('open','partially_settled')",false),("inbound_lines","仍有在途数量的采购行","SELECT count(*) FROM purchase_order_lines l JOIN purchase_orders o ON o.id=l.purchase_order_id WHERE o.supplier_id=$1 AND o.lifecycle_status='confirmed' AND l.ordered_quantity>l.received_quantity+l.cancelled_quantity",true)],
CoreMasterType::Warehouse=>&[("stock","存在余额的库存记录","SELECT count(*) FROM inventory_balances WHERE warehouse_id=$1 AND (on_hand_quantity<>0 OR reserved_quantity<>0 OR quarantined_quantity<>0)",true),("sales_demand","未完成销售订单行","SELECT count(*) FROM sales_order_lines l JOIN sales_orders o ON o.id=l.sales_order_id WHERE l.warehouse_id=$1 AND o.lifecycle_status IN ('draft','confirmed') AND l.ordered_quantity>l.shipped_quantity+l.cancelled_quantity",true),("purchase_inbound","未完成采购订单行","SELECT count(*) FROM purchase_order_lines l JOIN purchase_orders o ON o.id=l.purchase_order_id WHERE l.warehouse_id=$1 AND o.lifecycle_status IN ('draft','confirmed') AND l.ordered_quantity>l.received_quantity+l.cancelled_quantity",true),("inventory_counts","进行中的盘点任务","SELECT count(*) FROM inventory_count_tasks WHERE warehouse_id=$1 AND status IN ('counting','counted')",true)]};
    // All impact counts use one statement snapshot on the caller's transaction.
    let sql = format!(
        "SELECT ARRAY[{}]",
        queries
            .iter()
            .map(|(_, _, sql, _)| format!("({sql})"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let counts: Vec<i64> = sqlx::query_scalar(AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(&mut *connection)
        .await?;
    let out = queries
        .iter()
        .zip(counts)
        .map(|((code, label, _, blocking), count)| ImpactItem {
            code: (*code).into(),
            label: (*label).into(),
            count,
            blocking: *blocking,
        })
        .collect();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_lowercase_code() {
        let i = SaveCoreMasterData {
            resource_type: "warehouse".into(),
            code: "bad".into(),
            name: "A".into(),
            legal_entity_id: Some(Uuid::nil()),
            business_unit_id: Some(Uuid::nil()),
            parent_business_unit_id: None,
            country_code: None,
            functional_currency: None,
            registration_number: None,
            address: None,
            credit_currency: None,
            credit_limit_minor: None,
            payment_terms_days: None,
            expected_version: None,
        };
        assert!(validate(&i, CoreMasterType::Warehouse, false).is_err());
    }
}
