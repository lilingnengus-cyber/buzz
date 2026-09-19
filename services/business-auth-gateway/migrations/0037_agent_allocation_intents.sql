CREATE TABLE business_agent_allocation_intents (
 id uuid PRIMARY KEY,
 kind text NOT NULL CHECK(kind IN ('receivable_allocation_intent','payable_allocation_intent')),
 source_document_id uuid NOT NULL,
 input jsonb NOT NULL,
 snapshot jsonb NOT NULL,
 created_by_user_id uuid NOT NULL REFERENCES enterprise_users(id),
 idempotency_key text NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 expires_at timestamptz NOT NULL DEFAULT now()+interval '30 minutes',
 trace_id uuid NOT NULL,
 UNIQUE(created_by_user_id,idempotency_key)
);
CREATE TRIGGER business_agent_allocation_intents_immutable BEFORE UPDATE OR DELETE ON business_agent_allocation_intents FOR EACH ROW EXECUTE FUNCTION deny_business_document_approval_vote_mutation();
ALTER TABLE business_document_approval_requests
 DROP CONSTRAINT business_document_approval_requests_document_type_check,
 ADD CONSTRAINT business_document_approval_requests_document_type_check CHECK(document_type IN ('sales_order','purchase_order','shipment','goods_receipt','inventory_opening','customer_receipt','supplier_payment','receivable_allocation_intent','payable_allocation_intent')),
 DROP CONSTRAINT business_document_approval_requests_action_code_check,
 ADD CONSTRAINT business_document_approval_requests_action_code_check CHECK(action_code IN ('sales_order:confirm','purchase_order:confirm','shipment:confirm','goods_receipt:confirm','inventory_opening:post','customer_receipt:confirm','supplier_payment:confirm','receivable_allocation:create','payable_allocation:create'));

ALTER TABLE agent_read_delegations DROP CONSTRAINT agent_read_delegations_scopes_check,
 ADD CONSTRAINT agent_read_delegations_scopes_check CHECK(cardinality(scopes) BETWEEN 1 AND 48),
 DROP CONSTRAINT agent_read_delegations_approval_document_type_check,
 ADD CONSTRAINT agent_read_delegations_approval_document_type_check CHECK(approval_document_type IS NULL OR approval_document_type IN ('sales_order','purchase_order','shipment','goods_receipt','inventory_opening','customer_receipt','supplier_payment','receivable_allocation_intent','payable_allocation_intent'));
INSERT INTO business_iam.permissions(id,capability,resource_type,action,obligations,risk_level)
SELECT gen_random_uuid(),resource||':'||action,resource,action,
 CASE WHEN action='approve' THEN '["fresh_signed_chat_command"]'::jsonb ELSE '[]'::jsonb END,
 CASE WHEN action='approve' THEN 'high' ELSE 'medium' END
FROM (VALUES ('receivable_allocation_intent','create'),('payable_allocation_intent','create'),('receivable_allocation_intent','approve'),('payable_allocation_intent','approve')) AS capability(resource,action)
ON CONFLICT(capability) DO NOTHING;
-- Registration only: no automatic grants or policies.
