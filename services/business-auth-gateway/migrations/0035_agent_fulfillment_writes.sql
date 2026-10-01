-- Register capabilities only; operators grant these within existing business scopes.
ALTER TABLE agent_read_delegations DROP CONSTRAINT agent_read_delegations_scopes_check,
 ADD CONSTRAINT agent_read_delegations_scopes_check CHECK(cardinality(scopes) BETWEEN 1 AND 32),
 DROP CONSTRAINT agent_read_delegations_approval_document_type_check,
 ADD CONSTRAINT agent_read_delegations_approval_document_type_check CHECK(approval_document_type IS NULL OR approval_document_type IN ('sales_order','purchase_order','shipment','goods_receipt','inventory_opening'));
ALTER TABLE business_document_approval_requests
 DROP CONSTRAINT business_document_approval_requests_document_type_check,
 ADD CONSTRAINT business_document_approval_requests_document_type_check CHECK(document_type IN ('sales_order','purchase_order','shipment','goods_receipt','inventory_opening')),
 DROP CONSTRAINT business_document_approval_requests_action_code_check,
 ADD CONSTRAINT business_document_approval_requests_action_code_check CHECK(action_code IN ('sales_order:confirm','purchase_order:confirm','shipment:confirm','goods_receipt:confirm','inventory_opening:post'));
INSERT INTO business_iam.permissions(id,capability,resource_type,action,obligations,risk_level)
SELECT gen_random_uuid(), resource||':'||action,resource,action,
 CASE WHEN action='approve' THEN '["fresh_signed_chat_command"]'::jsonb ELSE '[]'::jsonb END,
 CASE WHEN action='approve' THEN 'high' ELSE 'medium' END
FROM (VALUES ('sales_order','update_draft'),('purchase_order','update_draft'),('inventory_opening','create'),('shipment','read'),('goods_receipt','read'),('shipment','approve'),('goods_receipt','approve'),('inventory_opening','approve')) AS capability(resource,action)
ON CONFLICT(capability) DO NOTHING;
-- No automatic document-approval policies or grants. Missing policy fails closed.

-- A changed inventory/cost preview can be approved again without editing the draft.
ALTER TABLE business_document_approval_requests
 DROP CONSTRAINT business_document_approval_re_document_type_document_id_exp_key;
CREATE UNIQUE INDEX business_document_approval_preview_key
 ON business_document_approval_requests(document_type,document_id,expected_version,preview_hash);
