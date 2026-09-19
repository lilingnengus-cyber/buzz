-- Return counts, amounts, losses and scrap are net activity in each business month.
-- A later correction never erases the original month; correction-only months can be negative.
CREATE OR REPLACE VIEW return_operating_metrics AS
WITH shipment_period AS (
    SELECT legal_entity_id,currency,date_trunc('month',shipment_date)::date management_period,
           sum(sales_amount)::numeric(24,6) shipped_sales_amount
    FROM shipments WHERE status='confirmed' GROUP BY 1,2,3
), sales_activity AS (
    SELECT legal_entity_id,currency,return_date business_date,1::bigint count_delta,
           sales_amount amount,GREATEST(sales_amount-cost_amount+scrap_cost_amount,0) loss,scrap_cost_amount scrap
    FROM sales_returns WHERE status IN ('confirmed','reversed')
    UNION ALL
    SELECT r.legal_entity_id,r.currency,(e.payload->>'reversalDate')::date,-1::bigint,
           -r.sales_amount,-GREATEST(r.sales_amount-r.cost_amount+r.scrap_cost_amount,0),-r.scrap_cost_amount
    FROM sales_return_events e JOIN sales_returns r ON r.id=e.sales_return_id WHERE e.event_type='reversed'
), sales_return_period AS (
    SELECT legal_entity_id,currency,date_trunc('month',business_date)::date management_period,
           sum(count_delta)::bigint return_count,sum(amount)::numeric(24,6) return_sales_amount,
           sum(loss)::numeric(24,6) return_loss_amount,sum(scrap)::numeric(24,6) scrap_cost_amount
    FROM sales_activity GROUP BY 1,2,3
), receipt_period AS (
    SELECT legal_entity_id,currency,date_trunc('month',receipt_date)::date management_period,
           sum(gross_amount)::numeric(24,6) received_purchase_amount
    FROM goods_receipts WHERE status='confirmed' GROUP BY 1,2,3
), purchase_activity AS (
    SELECT legal_entity_id,currency,return_date business_date,1::bigint count_delta,gross_amount amount
    FROM purchase_returns WHERE status IN ('confirmed','reversed')
    UNION ALL
    SELECT r.legal_entity_id,r.currency,(e.payload->>'reversalDate')::date,-1::bigint,-r.gross_amount
    FROM purchase_return_events e JOIN purchase_returns r ON r.id=e.purchase_return_id WHERE e.event_type='reversed'
), purchase_return_period AS (
    SELECT legal_entity_id,currency,date_trunc('month',business_date)::date management_period,
           sum(count_delta)::bigint return_count,sum(amount)::numeric(24,6) return_purchase_amount
    FROM purchase_activity GROUP BY 1,2,3
), keys AS (
    SELECT legal_entity_id,currency,management_period FROM shipment_period
    UNION SELECT legal_entity_id,currency,management_period FROM sales_return_period
    UNION SELECT legal_entity_id,currency,management_period FROM receipt_period
    UNION SELECT legal_entity_id,currency,management_period FROM purchase_return_period
)
SELECT k.legal_entity_id,k.currency::text currency,k.management_period,
       COALESCE(s.shipped_sales_amount,0)::numeric(24,6) shipped_sales_amount,
       COALESCE(sr.return_count,0)::bigint sales_return_count,
       COALESCE(sr.return_sales_amount,0)::numeric(24,6) sales_return_amount,
       CASE WHEN COALESCE(s.shipped_sales_amount,0)=0 THEN NULL
            ELSE (COALESCE(sr.return_sales_amount,0)/s.shipped_sales_amount)::numeric(24,8) END sales_return_rate,
       COALESCE(sr.return_loss_amount,0)::numeric(24,6) return_loss_amount,
       COALESCE(sr.scrap_cost_amount,0)::numeric(24,6) scrap_cost_amount,
       COALESCE(r.received_purchase_amount,0)::numeric(24,6) received_purchase_amount,
       COALESCE(pr.return_count,0)::bigint purchase_return_count,
       COALESCE(pr.return_purchase_amount,0)::numeric(24,6) purchase_return_amount,
       CASE WHEN COALESCE(r.received_purchase_amount,0)=0 THEN NULL
            ELSE (COALESCE(pr.return_purchase_amount,0)/r.received_purchase_amount)::numeric(24,8) END purchase_return_rate
FROM keys k
LEFT JOIN shipment_period s USING(legal_entity_id,currency,management_period)
LEFT JOIN sales_return_period sr USING(legal_entity_id,currency,management_period)
LEFT JOIN receipt_period r USING(legal_entity_id,currency,management_period)
LEFT JOIN purchase_return_period pr USING(legal_entity_id,currency,management_period);
