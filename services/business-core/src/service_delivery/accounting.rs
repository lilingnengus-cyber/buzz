use super::*;
use crate::b2::common::{money, next_number, record};
use chrono::{Duration, NaiveDate};
use rust_decimal::Decimal;
use sqlx::{Postgres, Transaction};
impl ServiceDelivery {
    pub(super) async fn recognize(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        trace: Uuid,
        project: Uuid,
        acceptance: Uuid,
        date: NaiveDate,
    ) -> Result<Value, DomainError> {
        let source: Option<Uuid> =
            sqlx::query_scalar("SELECT sales_order_line_id FROM service_projects WHERE id=$1")
                .bind(project)
                .fetch_one(&mut **tx)
                .await?;
        let line_id = source.ok_or_else(|| {
            DomainError::Invalid("请先关联已确认的服务销售订单，再验收通过并自动记账".into())
        })?;
        let order_id: Uuid =
            sqlx::query_scalar("SELECT sales_order_id FROM sales_order_lines WHERE id=$1")
                .bind(line_id)
                .fetch_one(&mut **tx)
                .await?;
        let order = sqlx::query("SELECT * FROM sales_orders WHERE id=$1 FOR UPDATE")
            .bind(order_id)
            .fetch_one(&mut **tx)
            .await?;
        authorize(
            &self.store,
            actor,
            "service_delivery:accept",
            Some(order.get("legal_entity_id")),
            None,
            Some(order.get("customer_id")),
            order.get("brand_id"),
            Some(order.get("business_unit_id")),
        )
        .await?;
        if order.get::<String, _>("lifecycle_status") != "confirmed"
            || order.get::<String, _>("hold_status") != "none"
        {
            return Err(DomainError::Invalid(
                "订单须已确认且未冻结，才能验收记账".into(),
            ));
        }
        if date < order.get::<NaiveDate, _>("order_date") {
            return Err(DomainError::Invalid("验收日期不能早于订单日期".into()));
        }
        let line=sqlx::query("SELECT l.*,p.category_id,COALESCE(l.brand_id,p.brand_id) effective_brand_id FROM sales_order_lines l JOIN business_skus s ON s.id=l.sku_id JOIN business_products p ON p.id=s.product_id WHERE l.id=$1 FOR UPDATE OF l").bind(line_id).fetch_one(&mut **tx).await?;
        authorize(
            &self.store,
            actor,
            "service_delivery:accept",
            Some(order.get("legal_entity_id")),
            None,
            Some(order.get("customer_id")),
            line.get("effective_brand_id"),
            Some(line.get("business_unit_id")),
        )
        .await?;
        if line.get::<String, _>("service_kind") == "goods"
            || line.get::<Decimal, _>("cancelled_quantity") != Decimal::ZERO
            || line.get::<Decimal, _>("service_fulfilled_quantity") != Decimal::ZERO
        {
            return Err(DomainError::Invalid("服务行已取消或已完成记账".into()));
        }
        let id = Uuid::new_v4();
        let amount: Decimal = line.get("gross_amount");
        let number = next_number(
            tx,
            "receivable",
            &self.receivable_prefix,
            id,
            crate::numbering::NumberingContext::new(
                order.get("legal_entity_id"),
                Some(order.get("business_unit_id")),
            ),
        )
        .await?;
        let terms: i32 = order.get("payment_terms_days");
        let due = date
            .checked_add_signed(Duration::days(i64::from(terms)))
            .ok_or_else(|| DomainError::Invalid("到期日超出范围".into()))?;
        sqlx::query("INSERT INTO trade_receivables(id,receivable_number,legal_entity_id,customer_id,sales_order_id,service_project_id,service_acceptance_id,currency,original_amount,open_amount,recognized_at,due_date,payment_terms_days,payment_terms_snapshot,trace_id,status) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$9,now(),$10,$11,$12,$13,CASE WHEN $9=0 THEN 'settled' ELSE 'open' END)")
            .bind(id).bind(&number).bind(order.get::<Uuid,_>("legal_entity_id")).bind(order.get::<Uuid,_>("customer_id")).bind(order_id).bind(project).bind(acceptance).bind(order.get::<String,_>("currency")).bind(amount).bind(due).bind(terms).bind(json!({"days":terms,"basis":"service_acceptance_date","acceptedOn":date})).bind(trace).execute(&mut **tx).await?;
        sqlx::query("INSERT INTO trade_receivable_events(id,receivable_id,event_type,amount,payload,actor_user_id,trace_id) VALUES($1,$2,'created',$3,$4,$5,$6)").bind(Uuid::new_v4()).bind(id).bind(amount).bind(json!({"serviceProjectId":project,"acceptanceId":acceptance,"dueDate":due})).bind(actor).bind(trace).execute(&mut **tx).await?;
        sqlx::query(
            "UPDATE sales_order_lines SET service_fulfilled_quantity=ordered_quantity WHERE id=$1",
        )
        .bind(line_id)
        .execute(&mut **tx)
        .await?;
        let complete:bool=sqlx::query_scalar("SELECT bool_and(shipped_quantity+service_fulfilled_quantity+cancelled_quantity=ordered_quantity) FROM sales_order_lines WHERE sales_order_id=$1").bind(order_id).fetch_one(&mut **tx).await?;
        sqlx::query("UPDATE sales_orders SET fulfillment_status=CASE WHEN $2 THEN 'fulfilled' ELSE 'partially_fulfilled' END,lifecycle_status=CASE WHEN $2 THEN 'completed' ELSE lifecycle_status END,completed_at=CASE WHEN $2 THEN now() ELSE completed_at END,updated_by_user_id=$3,trace_id=$4 WHERE id=$1").bind(order_id).bind(complete).bind(actor).bind(trace).execute(&mut **tx).await?;
        sqlx::query("INSERT INTO profit_facts(id,metric_type,direction,amount,currency,quantity,legal_entity_id,sales_order_id,sales_order_line_id,customer_id,sku_id,product_category_id,brand_id,salesperson_user_id,business_unit_id,department_id,business_date,management_period,source_system,source_type,source_id,source_line_id,source_event_id,source_event_version,data_as_of,trace_id) VALUES($1,'net_revenue','normal',$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,'business_core_b2','service_acceptance',$17,$7,$18,1,now(),$19)")
            .bind(Uuid::new_v4()).bind(money(line.get("net_amount"))).bind(order.get::<String,_>("currency")).bind(line.get::<Decimal,_>("ordered_quantity")).bind(order.get::<Uuid,_>("legal_entity_id")).bind(order_id).bind(line_id).bind(order.get::<Uuid,_>("customer_id")).bind(line.get::<Uuid,_>("sku_id")).bind(line.get::<Uuid,_>("category_id")).bind(line.get::<Option<Uuid>,_>("effective_brand_id")).bind(order.get::<Uuid,_>("salesperson_user_id")).bind(line.get::<Uuid,_>("business_unit_id")).bind(line.get::<Option<Uuid>,_>("department_id")).bind(date).bind(date.format("%Y-%m").to_string()).bind(project).bind(acceptance).bind(trace).execute(&mut **tx).await?;
        record(tx,trace,actor,"SERVICE_RECEIVABLE_CREATED","trade_receivable_created","trade_receivable",id,json!({"receivableNumber":number,"serviceProjectId":project,"amount":amount.to_string()})).await?;
        Ok(
            json!({"id":id,"number":number,"amount":amount.to_string(),"currency":order.get::<String,_>("currency"),"dueDate":due}),
        )
    }
}
