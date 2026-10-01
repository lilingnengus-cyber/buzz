CREATE TABLE business_agent_operating_snapshot_intents (
 id uuid PRIMARY KEY,
 kind text NOT NULL CHECK(kind IN ('operating_report_snapshot_intent')),
 input jsonb NOT NULL,
 snapshot jsonb NOT NULL,
 created_by_user_id uuid NOT NULL REFERENCES enterprise_users(id),
 idempotency_key text NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 expires_at timestamptz NOT NULL DEFAULT now()+interval '30 minutes',
 trace_id uuid NOT NULL,
 UNIQUE(created_by_user_id,idempotency_key)
);
CREATE TRIGGER business_agent_operating_snapshot_intents_immutable
 BEFORE UPDATE OR DELETE ON business_agent_operating_snapshot_intents
 FOR EACH ROW EXECUTE FUNCTION deny_business_document_approval_vote_mutation();
ALTER TABLE business_document_approval_requests DROP CONSTRAINT business_document_approval_requests_document_type_check, ADD CONSTRAINT business_document_approval_requests_document_type_check CHECK(document_type IN ('sales_order','purchase_order','shipment','goods_receipt','inventory_opening','customer_receipt','supplier_payment','receivable_allocation_intent','payable_allocation_intent','customer_receipt_reversal_intent','supplier_payment_reversal_intent','receivable_allocation_reversal_intent','payable_allocation_reversal_intent','sales_order_cancellation_intent','purchase_order_cancellation_intent','shipment_reversal_intent','goods_receipt_reversal_intent','inventory_opening_reversal_intent','sales_return','purchase_return','sales_return_inspection_intent','purchase_return_dispatch_intent','purchase_return_acknowledgment_intent','sales_return_cancellation_intent','purchase_return_cancellation_intent','sales_return_reversal_intent','purchase_return_reversal_intent','inventory_count_creation_intent','inventory_count_submission_intent','inventory_count_posting_intent','inventory_count_cancellation_intent','crm_creation_intent','crm_update_intent','crm_followup_intent','core_master_creation_intent','core_master_update_intent','product_master_creation_intent','product_master_update_intent','core_master_status_intent','product_master_status_intent','sales_order_hold_intent','sales_order_release_hold_intent','management_report_snapshot_intent','operating_report_snapshot_intent'));
ALTER TABLE agent_read_delegations DROP CONSTRAINT agent_read_delegations_approval_document_type_check, ADD CONSTRAINT agent_read_delegations_approval_document_type_check CHECK(approval_document_type IS NULL OR approval_document_type IN ('sales_order','purchase_order','shipment','goods_receipt','inventory_opening','customer_receipt','supplier_payment','receivable_allocation_intent','payable_allocation_intent','customer_receipt_reversal_intent','supplier_payment_reversal_intent','receivable_allocation_reversal_intent','payable_allocation_reversal_intent','sales_order_cancellation_intent','purchase_order_cancellation_intent','shipment_reversal_intent','goods_receipt_reversal_intent','inventory_opening_reversal_intent','sales_return','purchase_return','sales_return_inspection_intent','purchase_return_dispatch_intent','purchase_return_acknowledgment_intent','sales_return_cancellation_intent','purchase_return_cancellation_intent','sales_return_reversal_intent','purchase_return_reversal_intent','inventory_count_creation_intent','inventory_count_submission_intent','inventory_count_posting_intent','inventory_count_cancellation_intent','crm_creation_intent','crm_update_intent','crm_followup_intent','core_master_creation_intent','core_master_update_intent','product_master_creation_intent','product_master_update_intent','core_master_status_intent','product_master_status_intent','sales_order_hold_intent','sales_order_release_hold_intent','management_report_snapshot_intent','operating_report_snapshot_intent'));
INSERT INTO business_iam.permissions(id,capability,resource_type,action,obligations,risk_level)
SELECT gen_random_uuid(),kind||':'||action,kind,action,
CASE WHEN action='approve' THEN '["fresh_signed_chat_command"]'::jsonb ELSE '[]'::jsonb END,'high'
FROM (VALUES ('operating_report_snapshot_intent')) kinds(kind)
CROSS JOIN (VALUES ('create'),('approve')) actions(action)
ON CONFLICT(capability) DO NOTHING;
-- No automatic grants or approval policies.
