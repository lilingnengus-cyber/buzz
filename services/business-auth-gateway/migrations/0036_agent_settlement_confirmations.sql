-- Business receipt/payment recording only; no external banking capability.
ALTER TABLE business_document_approval_requests
 DROP CONSTRAINT business_document_approval_requests_document_type_check,
 ADD CONSTRAINT business_document_approval_requests_document_type_check CHECK(document_type IN ('sales_order','purchase_order','shipment','goods_receipt','inventory_opening','customer_receipt','supplier_payment')),
 DROP CONSTRAINT business_document_approval_requests_action_code_check,
 ADD CONSTRAINT business_document_approval_requests_action_code_check CHECK(action_code IN ('sales_order:confirm','purchase_order:confirm','shipment:confirm','goods_receipt:confirm','inventory_opening:post','customer_receipt:confirm','supplier_payment:confirm'));
ALTER TABLE agent_read_delegations
 DROP CONSTRAINT agent_read_delegations_approval_document_type_check,
 ADD CONSTRAINT agent_read_delegations_approval_document_type_check CHECK(approval_document_type IS NULL OR approval_document_type IN ('sales_order','purchase_order','shipment','goods_receipt','inventory_opening','customer_receipt','supplier_payment'));
INSERT INTO business_iam.permissions(id,capability,resource_type,action,obligations,risk_level)
SELECT gen_random_uuid(),resource||':'||action,resource,action,
 CASE WHEN action='approve' THEN '["fresh_signed_chat_command"]'::jsonb ELSE '[]'::jsonb END,
 CASE WHEN action='approve' THEN 'high' ELSE 'medium' END
FROM (VALUES ('customer_receipt','read'),('supplier_payment','read'),('customer_receipt','approve'),('supplier_payment','approve')) AS capability(resource,action)
ON CONFLICT(capability) DO NOTHING;
-- No automatic grants or approval policies.

