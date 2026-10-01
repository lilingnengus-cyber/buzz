CREATE TABLE business_agent_master_intents (
 id uuid PRIMARY KEY,
 kind text NOT NULL CHECK(kind IN ('core_master_creation_intent','core_master_update_intent','product_master_creation_intent','product_master_update_intent')),
 input jsonb NOT NULL,
 snapshot jsonb NOT NULL,
 created_by_user_id uuid NOT NULL REFERENCES enterprise_users(id),
 idempotency_key text NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 expires_at timestamptz NOT NULL DEFAULT now()+interval '30 minutes',
 trace_id uuid NOT NULL,
 UNIQUE(created_by_user_id,idempotency_key)
);
CREATE TRIGGER business_agent_master_intents_immutable
 BEFORE UPDATE OR DELETE ON business_agent_master_intents
 FOR EACH ROW EXECUTE FUNCTION deny_business_document_approval_vote_mutation();
ALTER TABLE business_document_approval_requests DROP CONSTRAINT business_document_approval_requests_document_type_check,
 ADD CONSTRAINT business_document_approval_requests_document_type_check CHECK(document_type IN ('sales_order','purchase_order','shipment','goods_receipt','inventory_opening','customer_receipt','supplier_payment','receivable_allocation_intent','payable_allocation_intent','customer_receipt_reversal_intent','supplier_payment_reversal_intent','receivable_allocation_reversal_intent','payable_allocation_reversal_intent','sales_order_cancellation_intent','purchase_order_cancellation_intent','shipment_reversal_intent','goods_receipt_reversal_intent','inventory_opening_reversal_intent','sales_return','purchase_return','sales_return_inspection_intent','purchase_return_dispatch_intent','purchase_return_acknowledgment_intent','sales_return_cancellation_intent','purchase_return_cancellation_intent','sales_return_reversal_intent','purchase_return_reversal_intent','inventory_count_creation_intent','inventory_count_submission_intent','inventory_count_posting_intent','inventory_count_cancellation_intent','crm_creation_intent','crm_update_intent','crm_followup_intent','core_master_creation_intent','core_master_update_intent','product_master_creation_intent','product_master_update_intent')),
 DROP CONSTRAINT business_document_approval_requests_action_code_check,
 ADD CONSTRAINT business_document_approval_requests_action_code_check CHECK(action_code IN ('sales_order:confirm','purchase_order:confirm','shipment:confirm','goods_receipt:confirm','inventory_opening:post','customer_receipt:confirm','supplier_payment:confirm','receivable_allocation:create','payable_allocation:create','customer_receipt:reverse','supplier_payment:reverse','receivable_allocation:reverse','payable_allocation:reverse','sales_order:cancel','purchase_order:cancel_remaining','shipment:reverse','goods_receipt:reverse','inventory_opening:reverse','inventory_opening:create','crm:manage','business_master_data:manage','business_product_master:manage'));
ALTER TABLE agent_read_delegations DROP CONSTRAINT agent_read_delegations_approval_document_type_check,
 ADD CONSTRAINT agent_read_delegations_approval_document_type_check CHECK(approval_document_type IS NULL OR approval_document_type IN ('sales_order','purchase_order','shipment','goods_receipt','inventory_opening','customer_receipt','supplier_payment','receivable_allocation_intent','payable_allocation_intent','customer_receipt_reversal_intent','supplier_payment_reversal_intent','receivable_allocation_reversal_intent','payable_allocation_reversal_intent','sales_order_cancellation_intent','purchase_order_cancellation_intent','shipment_reversal_intent','goods_receipt_reversal_intent','inventory_opening_reversal_intent','sales_return','purchase_return','sales_return_inspection_intent','purchase_return_dispatch_intent','purchase_return_acknowledgment_intent','sales_return_cancellation_intent','purchase_return_cancellation_intent','sales_return_reversal_intent','purchase_return_reversal_intent','inventory_count_creation_intent','inventory_count_submission_intent','inventory_count_posting_intent','inventory_count_cancellation_intent','crm_creation_intent','crm_update_intent','crm_followup_intent','core_master_creation_intent','core_master_update_intent','product_master_creation_intent','product_master_update_intent'));


INSERT INTO business_iam.permissions(id,capability,resource_type,action,obligations,risk_level)
SELECT gen_random_uuid(),kind||':'||action,kind,action,
CASE WHEN action='approve' THEN '["fresh_signed_chat_command"]'::jsonb ELSE '[]'::jsonb END,'high'
FROM (VALUES ('core_master_creation_intent'),('core_master_update_intent'),('product_master_creation_intent'),('product_master_update_intent')) AS kinds(kind)
CROSS JOIN (VALUES ('create'),('approve')) AS actions(action)
ON CONFLICT(capability) DO NOTHING;
-- No automatic grants or approval policies. Guarded status execution is not enabled.
