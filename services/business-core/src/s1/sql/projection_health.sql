WITH visible_sources AS (
  SELECT s.id,'shipment'::text kind FROM shipments s
  JOIN sales_orders o ON o.id=s.sales_order_id
  WHERE s.legal_entity_id=ANY($1) AND s.customer_id=ANY($2)
    AND s.warehouse_id=ANY($3) AND (o.brand_id IS NULL OR o.brand_id=ANY($4))
    AND o.business_unit_id=ANY($5)
  UNION ALL
  SELECT r.id,'sales_return'::text kind FROM sales_returns r
  JOIN sales_orders o ON o.id=r.sales_order_id
  WHERE r.legal_entity_id=ANY($1) AND r.customer_id=ANY($2)
    AND r.warehouse_id=ANY($3) AND (o.brand_id IS NULL OR o.brand_id=ANY($4))
    AND o.business_unit_id=ANY($5)
), visible_topics AS (
  SELECT id,kind,unnest(CASE kind
    WHEN 'shipment' THEN ARRAY['shipment_confirmed','shipment_reversed']
    ELSE ARRAY['sales_return_confirmed','sales_return_reversed'] END) topic
  FROM visible_sources
)
SELECT o.last_outbox_created_at,o.last_fact_sequence,o.updated_at,
  (SELECT count(*) FROM business_core_outbox e
   WHERE (o.last_outbox_created_at IS NULL OR
     (e.created_at,e.id)>(o.last_outbox_created_at,o.last_outbox_event_id))
   AND EXISTS(SELECT 1 FROM visible_topics v WHERE v.id::text=e.aggregate_id
     AND v.kind=e.aggregate_type AND v.topic=e.topic)) pending_events,
  (SELECT count(*) FROM profit_projection_failures f
   WHERE f.status='pending' AND EXISTS(SELECT 1 FROM visible_topics v
     WHERE v.id=f.aggregate_id AND v.topic=f.topic)) pending_failures
FROM (SELECT 1) anchor
LEFT JOIN profit_projection_offsets o ON o.consumer_name='profit_projection_v1'
