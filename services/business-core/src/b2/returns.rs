mod confirmation;
mod draft;
use super::{
    common::{
        authorize, begin_idempotent, finish_idempotent, money, next_number, record, request_hash,
        DomainError,
    },
    model::{CommandResult, DecimalString, VersionCommand},
};
use crate::store::PgStore;
use chrono::{NaiveDate, Utc};
pub use draft::ReplaceReturnDraft;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReturnLineInput {
    pub source_line_id: Uuid,
    pub quantity: DecimalString,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateReturn {
    /// Current source version; required by the agent draft route. Omitted by legacy browser clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_source_version: Option<i64>,
    pub source_id: Uuid,
    pub return_date: NaiveDate,
    pub reason_code: String,
    #[serde(default)]
    pub business_note: Option<String>,
    pub lines: Vec<ReturnLineInput>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ReturnSummary {
    pub id: Uuid,
    pub return_number: String,
    pub source_id: Uuid,
    pub order_id: Uuid,
    pub partner_id: Uuid,
    pub warehouse_id: Uuid,
    pub return_date: NaiveDate,
    pub currency: String,
    pub reason_code: String,
    #[sqlx(try_from = "Decimal")]
    pub amount: DecimalString,
    pub status: String,
    pub workflow_status: String,
    pub version: i64,
    pub updated_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ReturnOptionLine {
    pub source_id: Uuid,
    pub source_number: String,
    pub order_id: Uuid,
    pub order_number: String,
    pub partner_id: Uuid,
    pub partner_code: String,
    pub partner_name: String,
    pub warehouse_id: Uuid,
    pub warehouse_code: String,
    pub warehouse_name: String,
    pub currency: String,
    pub source_line_id: Uuid,
    pub sku_id: Uuid,
    pub sku_code: String,
    pub sku_name: String,
    #[sqlx(try_from = "Decimal")]
    pub source_quantity: DecimalString,
    #[sqlx(try_from = "Decimal")]
    pub returned_quantity: DecimalString,
    #[sqlx(try_from = "Decimal")]
    pub returnable_quantity: DecimalString,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnOptions {
    pub can_create: bool,
    pub data_as_of: chrono::DateTime<Utc>,
    pub items: Vec<ReturnOptionLine>,
}

#[derive(Clone)]
pub struct ReturnService {
    store: PgStore,
    sales_prefix: String,
    purchase_prefix: String,
}

impl ReturnService {
    pub fn new(store: PgStore, sales_prefix: String, purchase_prefix: String) -> Self {
        Self {
            store,
            sales_prefix,
            purchase_prefix,
        }
    }

    /// Preview exact return confirmation effects and their transaction expectations.
    pub async fn confirmation_preview(
        &self,
        actor: Uuid,
        sales: bool,
        id: Uuid,
    ) -> Result<serde_json::Value, DomainError> {
        super::return_confirmation::preview(&self.store, actor, sales, id).await
    }

    pub async fn sales_options(&self, actor: Uuid) -> Result<ReturnOptions, DomainError> {
        let scope = authorize(
            &self.store,
            actor,
            "sales_order:read",
            None,
            None,
            None,
            None,
            None,
        )
        .await?;
        let rows = sqlx::query_as::<_, ReturnOptionLine>("SELECT s.id source_id,s.shipment_number source_number,s.sales_order_id order_id,o.order_number,s.customer_id partner_id,c.code partner_code,c.name partner_name,s.warehouse_id,w.code warehouse_code,w.name warehouse_name,s.currency::text currency,l.id source_line_id,l.sku_id,sku.code sku_code,sku.name sku_name,l.quantity source_quantity,COALESCE((SELECT sum(rl.quantity) FROM sales_return_lines rl JOIN sales_returns r ON r.id=rl.sales_return_id WHERE rl.shipment_line_id=l.id AND r.status IN ('draft','confirmed')),0) returned_quantity,GREATEST(l.quantity-COALESCE((SELECT sum(rl.quantity) FROM sales_return_lines rl JOIN sales_returns r ON r.id=rl.sales_return_id WHERE rl.shipment_line_id=l.id AND r.status IN ('draft','confirmed')),0),0) returnable_quantity FROM shipments s JOIN shipment_lines l ON l.shipment_id=s.id JOIN sales_orders o ON o.id=s.sales_order_id JOIN business_customers c ON c.id=s.customer_id JOIN business_warehouses w ON w.id=s.warehouse_id JOIN business_skus sku ON sku.id=l.sku_id WHERE s.status='confirmed' AND s.legal_entity_id=ANY($1) AND s.customer_id=ANY($2) AND s.warehouse_id=ANY($3) ORDER BY s.shipment_date DESC,s.shipment_number,l.id")
            .bind(scope.scopes.legal_entity_ids.into_iter().collect::<Vec<_>>())
            .bind(scope.scopes.customer_ids.into_iter().collect::<Vec<_>>())
            .bind(scope.scopes.warehouse_ids.into_iter().collect::<Vec<_>>())
            .fetch_all(self.store.pool()).await?;
        Ok(ReturnOptions {
            can_create: scope.permission_keys.contains("shipment:reverse"),
            data_as_of: Utc::now(),
            items: rows
                .into_iter()
                .filter(|row| row.returnable_quantity.0 > Decimal::ZERO)
                .collect(),
        })
    }

    pub async fn purchase_options(&self, actor: Uuid) -> Result<ReturnOptions, DomainError> {
        let scope = authorize(
            &self.store,
            actor,
            "goods_receipt:read",
            None,
            None,
            None,
            None,
            None,
        )
        .await?;
        let rows = sqlx::query_as::<_, ReturnOptionLine>("SELECT r.id source_id,r.goods_receipt_number source_number,r.purchase_order_id order_id,o.purchase_order_number order_number,r.supplier_id partner_id,s.code partner_code,s.name partner_name,r.warehouse_id,w.code warehouse_code,w.name warehouse_name,r.currency::text currency,l.id source_line_id,l.sku_id,sku.code sku_code,sku.name sku_name,l.received_quantity source_quantity,COALESCE((SELECT sum(rl.quantity) FROM purchase_return_lines rl JOIN purchase_returns pr ON pr.id=rl.purchase_return_id WHERE rl.goods_receipt_line_id=l.id AND pr.status IN ('draft','confirmed')),0) returned_quantity,GREATEST(l.received_quantity-COALESCE((SELECT sum(rl.quantity) FROM purchase_return_lines rl JOIN purchase_returns pr ON pr.id=rl.purchase_return_id WHERE rl.goods_receipt_line_id=l.id AND pr.status IN ('draft','confirmed')),0),0) returnable_quantity FROM goods_receipts r JOIN goods_receipt_lines l ON l.goods_receipt_id=r.id JOIN purchase_orders o ON o.id=r.purchase_order_id JOIN business_suppliers s ON s.id=r.supplier_id JOIN business_warehouses w ON w.id=r.warehouse_id JOIN business_skus sku ON sku.id=l.sku_id WHERE r.status='confirmed' AND r.legal_entity_id=ANY($1) AND r.supplier_id=ANY($2) AND r.warehouse_id=ANY($3) ORDER BY r.receipt_date DESC,r.goods_receipt_number,l.id")
            .bind(scope.scopes.legal_entity_ids.into_iter().collect::<Vec<_>>())
            .bind(scope.scopes.supplier_ids.into_iter().collect::<Vec<_>>())
            .bind(scope.scopes.warehouse_ids.into_iter().collect::<Vec<_>>())
            .fetch_all(self.store.pool()).await?;
        Ok(ReturnOptions {
            can_create: scope.permission_keys.contains("goods_receipt:reverse"),
            data_as_of: Utc::now(),
            items: rows
                .into_iter()
                .filter(|row| row.returnable_quantity.0 > Decimal::ZERO)
                .collect(),
        })
    }

    pub async fn sales_returns(
        &self,
        actor: Uuid,
        limit: i64,
    ) -> Result<Vec<ReturnSummary>, DomainError> {
        let scope = authorize(
            &self.store,
            actor,
            "sales_order:read",
            None,
            None,
            None,
            None,
            None,
        )
        .await?;
        sqlx::query_as::<_, ReturnSummary>("SELECT id,return_number,shipment_id source_id,sales_order_id order_id,customer_id partner_id,warehouse_id,return_date,currency::text currency,reason_code,sales_amount amount,status,inspection_status workflow_status,version,updated_at FROM sales_returns WHERE legal_entity_id=ANY($1) AND customer_id=ANY($2) AND warehouse_id=ANY($3) ORDER BY return_date DESC,return_number DESC LIMIT $4")
            .bind(scope.scopes.legal_entity_ids.into_iter().collect::<Vec<_>>()).bind(scope.scopes.customer_ids.into_iter().collect::<Vec<_>>()).bind(scope.scopes.warehouse_ids.into_iter().collect::<Vec<_>>()).bind(limit.clamp(1,500)).fetch_all(self.store.pool()).await.map_err(Into::into)
    }

    pub async fn purchase_returns(
        &self,
        actor: Uuid,
        limit: i64,
    ) -> Result<Vec<ReturnSummary>, DomainError> {
        let scope = authorize(
            &self.store,
            actor,
            "goods_receipt:read",
            None,
            None,
            None,
            None,
            None,
        )
        .await?;
        sqlx::query_as::<_, ReturnSummary>("SELECT id,return_number,goods_receipt_id source_id,purchase_order_id order_id,supplier_id partner_id,warehouse_id,return_date,currency::text currency,reason_code,gross_amount amount,status,logistics_status workflow_status,version,updated_at FROM purchase_returns WHERE legal_entity_id=ANY($1) AND supplier_id=ANY($2) AND warehouse_id=ANY($3) ORDER BY return_date DESC,return_number DESC LIMIT $4")
            .bind(scope.scopes.legal_entity_ids.into_iter().collect::<Vec<_>>()).bind(scope.scopes.supplier_ids.into_iter().collect::<Vec<_>>()).bind(scope.scopes.warehouse_ids.into_iter().collect::<Vec<_>>()).bind(limit.clamp(1,500)).fetch_all(self.store.pool()).await.map_err(Into::into)
    }

    fn validate_input(input: &CreateReturn) -> Result<(), DomainError> {
        if input.lines.is_empty() || input.lines.len() > 200 {
            return Err(DomainError::Invalid("return requires 1-200 lines".into()));
        }
        if input.reason_code.is_empty() || input.reason_code.len() > 64 {
            return Err(DomainError::Invalid("reasonCode is required".into()));
        }
        if input
            .business_note
            .as_ref()
            .is_some_and(|note| note.len() > 1000)
        {
            return Err(DomainError::Invalid("businessNote is too long".into()));
        }
        let mut seen = BTreeSet::new();
        for line in &input.lines {
            if !seen.insert(line.source_line_id) {
                return Err(DomainError::Invalid("duplicate return source line".into()));
            }
            line.quantity
                .positive("quantity")
                .map_err(DomainError::Invalid)?;
        }
        Ok(())
    }

    pub async fn create_sales_return(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        key: &str,
        input: &CreateReturn,
    ) -> Result<CommandResult, DomainError> {
        Self::validate_input(input)?;
        let source = sqlx::query(
            "SELECT legal_entity_id,warehouse_id,customer_id FROM shipments WHERE id=$1",
        )
        .bind(input.source_id)
        .fetch_optional(self.store.pool())
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
        authorize(
            &self.store,
            actor,
            "shipment:reverse",
            Some(source.get("legal_entity_id")),
            Some(source.get("warehouse_id")),
            Some(source.get("customer_id")),
            None,
            None,
        )
        .await?;
        super::return_scope::check_source(&self.store, actor, true, input.source_id).await?;
        let hash = request_hash(input)?;
        let mut tx = self.store.pool().begin().await?;
        if let Some(mut replay) =
            begin_idempotent::<CommandResult>(&mut tx, actor, "sales_return:create", key, &hash)
                .await?
        {
            replay.idempotent_replay = true;
            tx.commit().await?;
            return Ok(replay);
        }
        let shipment=sqlx::query("SELECT shipment_number,sales_order_id,legal_entity_id,warehouse_id,customer_id,currency::text,status,version FROM shipments WHERE id=$1 FOR UPDATE").bind(input.source_id).fetch_one(&mut *tx).await?;
        if input
            .expected_source_version
            .is_some_and(|version| version != shipment.get::<i64, _>("version"))
        {
            return Err(DomainError::VersionConflict);
        }
        if shipment.get::<String, _>("status") != "confirmed" {
            return Err(DomainError::Invalid(
                "sales return requires a confirmed shipment".into(),
            ));
        }
        let receivable_id: Uuid =
            sqlx::query_scalar("SELECT id FROM trade_receivables WHERE shipment_id=$1")
                .bind(input.source_id)
                .fetch_one(&mut *tx)
                .await?;
        for line in &input.lines {
            let source_line =
                sqlx::query("SELECT quantity FROM shipment_lines WHERE id=$1 AND shipment_id=$2")
                    .bind(line.source_line_id)
                    .bind(input.source_id)
                    .fetch_optional(&mut *tx)
                    .await?
                    .ok_or(DomainError::NotFoundOrForbidden)?;
            let allocated:Decimal=sqlx::query_scalar("SELECT COALESCE(sum(rl.quantity),0) FROM sales_return_lines rl JOIN sales_returns r ON r.id=rl.sales_return_id WHERE rl.shipment_line_id=$1 AND r.status IN ('draft','confirmed')").bind(line.source_line_id).fetch_one(&mut *tx).await?;
            if line.quantity.0 > source_line.get::<Decimal, _>("quantity") - allocated {
                return Err(DomainError::Invalid(
                    "sales return quantity exceeds shipment remainder".into(),
                ));
            }
        }
        let id = Uuid::new_v4();
        let business_unit_id: Uuid =
            sqlx::query_scalar("SELECT business_unit_id FROM sales_orders WHERE id=$1")
                .bind(shipment.get::<Uuid, _>("sales_order_id"))
                .fetch_one(&mut *tx)
                .await?;
        let number = next_number(
            &mut tx,
            "sales_return",
            &self.sales_prefix,
            id,
            crate::numbering::NumberingContext::new(
                shipment.get("legal_entity_id"),
                Some(business_unit_id),
            ),
        )
        .await?;
        sqlx::query("INSERT INTO sales_returns(id,return_number,shipment_id,sales_order_id,receivable_id,legal_entity_id,warehouse_id,customer_id,return_date,currency,reason_code,business_note,created_by_user_id,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)").bind(id).bind(&number).bind(input.source_id).bind(shipment.get::<Uuid,_>("sales_order_id")).bind(receivable_id).bind(shipment.get::<Uuid,_>("legal_entity_id")).bind(shipment.get::<Uuid,_>("warehouse_id")).bind(shipment.get::<Uuid,_>("customer_id")).bind(input.return_date).bind(shipment.get::<String,_>("currency")).bind(&input.reason_code).bind(&input.business_note).bind(actor).bind(trace_id).execute(&mut *tx).await?;
        for line in &input.lines {
            let sku: Uuid = sqlx::query_scalar("SELECT sku_id FROM shipment_lines WHERE id=$1")
                .bind(line.source_line_id)
                .fetch_one(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO sales_return_lines(id,sales_return_id,shipment_line_id,sku_id,quantity) VALUES($1,$2,$3,$4,$5)").bind(Uuid::new_v4()).bind(id).bind(line.source_line_id).bind(sku).bind(line.quantity.0).execute(&mut *tx).await?;
        }
        return_event(
            &mut tx,
            "sales",
            id,
            "created",
            1,
            (actor, trace_id),
            json!({"shipmentId":input.source_id}),
        )
        .await?;
        record(
            &mut tx,
            trace_id,
            actor,
            "SALES_RETURN_CREATED",
            "sales_return_created",
            "sales_return",
            id,
            json!({"returnNumber":number,"shipmentId":input.source_id}),
        )
        .await?;
        let result = CommandResult {
            id,
            number,
            status: "draft".into(),
            version: 1,
            trace_id,
            idempotent_replay: false,
        };
        finish_idempotent(&mut tx, actor, "sales_return:create", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn create_purchase_return(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        key: &str,
        input: &CreateReturn,
    ) -> Result<CommandResult, DomainError> {
        Self::validate_input(input)?;
        let source = sqlx::query(
            "SELECT legal_entity_id,warehouse_id,supplier_id FROM goods_receipts WHERE id=$1",
        )
        .bind(input.source_id)
        .fetch_optional(self.store.pool())
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
        crate::b3::common::authorize(
            &self.store,
            actor,
            "goods_receipt:reverse",
            Some(source.get("legal_entity_id")),
            Some(source.get("warehouse_id")),
            Some(source.get("supplier_id")),
            None,
            None,
        )
        .await?;
        super::return_scope::check_source(&self.store, actor, false, input.source_id).await?;
        let hash = request_hash(input)?;
        let mut tx = self.store.pool().begin().await?;
        if let Some(mut replay) =
            begin_idempotent::<CommandResult>(&mut tx, actor, "purchase_return:create", key, &hash)
                .await?
        {
            replay.idempotent_replay = true;
            tx.commit().await?;
            return Ok(replay);
        }
        let receipt=sqlx::query("SELECT purchase_order_id,legal_entity_id,warehouse_id,supplier_id,currency::text,status,version FROM goods_receipts WHERE id=$1 FOR UPDATE").bind(input.source_id).fetch_one(&mut *tx).await?;
        if input
            .expected_source_version
            .is_some_and(|version| version != receipt.get::<i64, _>("version"))
        {
            return Err(DomainError::VersionConflict);
        }
        if receipt.get::<String, _>("status") != "confirmed" {
            return Err(DomainError::Invalid(
                "purchase return requires a confirmed goods receipt".into(),
            ));
        }
        let payable_id: Uuid =
            sqlx::query_scalar("SELECT id FROM trade_payables WHERE goods_receipt_id=$1")
                .bind(input.source_id)
                .fetch_one(&mut *tx)
                .await?;
        for line in &input.lines {
            let source_line=sqlx::query("SELECT received_quantity FROM goods_receipt_lines WHERE id=$1 AND goods_receipt_id=$2").bind(line.source_line_id).bind(input.source_id).fetch_optional(&mut *tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
            let allocated:Decimal=sqlx::query_scalar("SELECT COALESCE(sum(rl.quantity),0) FROM purchase_return_lines rl JOIN purchase_returns r ON r.id=rl.purchase_return_id WHERE rl.goods_receipt_line_id=$1 AND r.status IN ('draft','confirmed')").bind(line.source_line_id).fetch_one(&mut *tx).await?;
            if line.quantity.0 > source_line.get::<Decimal, _>("received_quantity") - allocated {
                return Err(DomainError::Invalid(
                    "purchase return quantity exceeds receipt remainder".into(),
                ));
            }
        }
        let id = Uuid::new_v4();
        let business_unit_id: Uuid =
            sqlx::query_scalar("SELECT business_unit_id FROM purchase_orders WHERE id=$1")
                .bind(receipt.get::<Uuid, _>("purchase_order_id"))
                .fetch_one(&mut *tx)
                .await?;
        let number = next_number(
            &mut tx,
            "purchase_return",
            &self.purchase_prefix,
            id,
            crate::numbering::NumberingContext::new(
                receipt.get("legal_entity_id"),
                Some(business_unit_id),
            ),
        )
        .await?;
        sqlx::query("INSERT INTO purchase_returns(id,return_number,goods_receipt_id,purchase_order_id,payable_id,legal_entity_id,warehouse_id,supplier_id,return_date,currency,reason_code,business_note,created_by_user_id,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)").bind(id).bind(&number).bind(input.source_id).bind(receipt.get::<Uuid,_>("purchase_order_id")).bind(payable_id).bind(receipt.get::<Uuid,_>("legal_entity_id")).bind(receipt.get::<Uuid,_>("warehouse_id")).bind(receipt.get::<Uuid,_>("supplier_id")).bind(input.return_date).bind(receipt.get::<String,_>("currency")).bind(&input.reason_code).bind(&input.business_note).bind(actor).bind(trace_id).execute(&mut *tx).await?;
        for line in &input.lines {
            let sku: Uuid =
                sqlx::query_scalar("SELECT sku_id FROM goods_receipt_lines WHERE id=$1")
                    .bind(line.source_line_id)
                    .fetch_one(&mut *tx)
                    .await?;
            sqlx::query("INSERT INTO purchase_return_lines(id,purchase_return_id,goods_receipt_line_id,sku_id,quantity) VALUES($1,$2,$3,$4,$5)").bind(Uuid::new_v4()).bind(id).bind(line.source_line_id).bind(sku).bind(line.quantity.0).execute(&mut *tx).await?;
        }
        return_event(
            &mut tx,
            "purchase",
            id,
            "created",
            1,
            (actor, trace_id),
            json!({"goodsReceiptId":input.source_id}),
        )
        .await?;
        record(
            &mut tx,
            trace_id,
            actor,
            "PURCHASE_RETURN_CREATED",
            "purchase_return_created",
            "purchase_return",
            id,
            json!({"returnNumber":number,"goodsReceiptId":input.source_id}),
        )
        .await?;
        let result = CommandResult {
            id,
            number,
            status: "draft".into(),
            version: 1,
            trace_id,
            idempotent_replay: false,
        };
        finish_idempotent(&mut tx, actor, "purchase_return:create", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn cancel_sales_return(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        id: Uuid,
        key: &str,
        input: &VersionCommand,
    ) -> Result<CommandResult, DomainError> {
        self.cancel_return(actor, trace_id, id, key, input, "sales")
            .await
    }

    pub async fn cancel_purchase_return(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        id: Uuid,
        key: &str,
        input: &VersionCommand,
    ) -> Result<CommandResult, DomainError> {
        self.cancel_return(actor, trace_id, id, key, input, "purchase")
            .await
    }

    async fn cancel_return(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        id: Uuid,
        key: &str,
        input: &VersionCommand,
        side: &str,
    ) -> Result<CommandResult, DomainError> {
        let (pre_sql, select_sql, update_sql, permission, command, topic, event_code, entity_type) =
            if side == "sales" {
                (
                "SELECT legal_entity_id,warehouse_id,customer_id partner_id FROM sales_returns WHERE id=$1",
                "SELECT return_number,status,version FROM sales_returns WHERE id=$1 FOR UPDATE",
                "UPDATE sales_returns SET status='cancelled',version=$2,updated_at=now(),trace_id=$3 WHERE id=$1",
                "shipment:reverse",
                "sales_return:cancel",
                "sales_return_cancelled",
                "SALES_RETURN_CANCELLED",
                "sales_return",
            )
            } else {
                (
                "SELECT legal_entity_id,warehouse_id,supplier_id partner_id FROM purchase_returns WHERE id=$1",
                "SELECT return_number,status,version FROM purchase_returns WHERE id=$1 FOR UPDATE",
                "UPDATE purchase_returns SET status='cancelled',version=$2,updated_at=now(),trace_id=$3 WHERE id=$1",
                "goods_receipt:reverse",
                "purchase_return:cancel",
                "purchase_return_cancelled",
                "PURCHASE_RETURN_CANCELLED",
                "purchase_return",
            )
            };
        let pre = sqlx::query(pre_sql)
            .bind(id)
            .fetch_optional(self.store.pool())
            .await?
            .ok_or(DomainError::NotFoundOrForbidden)?;
        let scope = authorize(
            &self.store,
            actor,
            permission,
            Some(pre.get("legal_entity_id")),
            Some(pre.get("warehouse_id")),
            None,
            None,
            None,
        )
        .await?;
        let partner: Uuid = pre.get("partner_id");
        let allowed = if side == "sales" {
            scope.scopes.customer_ids.contains(&partner)
        } else {
            scope.scopes.supplier_ids.contains(&partner)
        };
        if !allowed {
            return Err(DomainError::NotFoundOrForbidden);
        }
        super::return_scope::check_return(&self.store, actor, side == "sales", id).await?;
        let hash = request_hash(input)?;
        let mut tx = self.store.pool().begin().await?;
        if let Some(mut replay) =
            begin_idempotent::<CommandResult>(&mut tx, actor, command, key, &hash).await?
        {
            replay.idempotent_replay = true;
            tx.commit().await?;
            return Ok(replay);
        }
        let ret = sqlx::query(select_sql).bind(id).fetch_one(&mut *tx).await?;
        check_draft(&ret, input.expected_version)?;
        let version = input.expected_version + 1;
        sqlx::query(update_sql)
            .bind(id)
            .bind(version)
            .bind(trace_id)
            .execute(&mut *tx)
            .await?;
        return_event(
            &mut tx,
            side,
            id,
            "cancelled",
            version,
            (actor, trace_id),
            json!({}),
        )
        .await?;
        record(
            &mut tx,
            trace_id,
            actor,
            event_code,
            topic,
            entity_type,
            id,
            json!({"version":version}),
        )
        .await?;
        let result = CommandResult {
            id,
            number: ret.get("return_number"),
            status: "cancelled".into(),
            version,
            trace_id,
            idempotent_replay: false,
        };
        finish_idempotent(&mut tx, actor, command, key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
}

fn check_draft(row: &sqlx::postgres::PgRow, expected: i64) -> Result<(), DomainError> {
    if row.get::<i64, _>("version") != expected {
        return Err(DomainError::VersionConflict);
    }
    if row.get::<String, _>("status") != "draft" {
        return Err(DomainError::Invalid(
            "only draft returns can be changed".into(),
        ));
    }
    Ok(())
}
fn balance_status(settled: Decimal, open: Decimal) -> &'static str {
    if open == Decimal::ZERO {
        "settled"
    } else if settled > Decimal::ZERO {
        "partially_settled"
    } else {
        "open"
    }
}

async fn confirmed_sales_amount(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    line: Uuid,
    current: Uuid,
) -> Result<Decimal, DomainError> {
    Ok(sqlx::query_scalar("SELECT COALESCE(sum(rl.sales_amount),0) FROM sales_return_lines rl JOIN sales_returns r ON r.id=rl.sales_return_id WHERE rl.shipment_line_id=$1 AND r.status='confirmed' AND r.id<>$2").bind(line).bind(current).fetch_one(&mut **tx).await?)
}
async fn confirmed_purchase_amounts(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    line: Uuid,
    current: Uuid,
) -> Result<(Decimal, Decimal, Decimal), DomainError> {
    let row=sqlx::query("SELECT COALESCE(sum(rl.net_amount),0) net,COALESCE(sum(rl.tax_amount),0) tax,COALESCE(sum(rl.gross_amount),0) gross FROM purchase_return_lines rl JOIN purchase_returns r ON r.id=rl.purchase_return_id WHERE rl.goods_receipt_line_id=$1 AND r.status='confirmed' AND r.id<>$2").bind(line).bind(current).fetch_one(&mut **tx).await?;
    Ok((row.get("net"), row.get("tax"), row.get("gross")))
}
async fn return_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    side: &str,
    id: Uuid,
    event: &str,
    version: i64,
    actor_trace: (Uuid, Uuid),
    payload: serde_json::Value,
) -> Result<(), DomainError> {
    let (actor, trace) = actor_trace;
    let sql = if side == "sales" {
        "INSERT INTO sales_return_events(id,sales_return_id,event_type,return_version,payload,actor_user_id,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7)"
    } else {
        "INSERT INTO purchase_return_events(id,purchase_return_id,event_type,return_version,payload,actor_user_id,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7)"
    };
    sqlx::query(sql)
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(event)
        .bind(version)
        .bind(payload)
        .bind(actor)
        .bind(trace)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balance_status_tracks_operational_settlement() {
        assert_eq!(balance_status(Decimal::ZERO, Decimal::new(100, 0)), "open");
        assert_eq!(
            balance_status(Decimal::new(20, 0), Decimal::new(80, 0)),
            "partially_settled"
        );
        assert_eq!(
            balance_status(Decimal::new(100, 0), Decimal::ZERO),
            "settled"
        );
    }
}
