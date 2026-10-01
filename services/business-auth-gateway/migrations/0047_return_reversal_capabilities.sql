INSERT INTO business_iam.permissions(id,capability,resource_type,action,obligations,risk_level)
SELECT gen_random_uuid(),resource||':'||action,resource,action,
CASE WHEN action='approve' THEN '["fresh_signed_chat_command"]'::jsonb ELSE '[]'::jsonb END,'high'
FROM (VALUES ('sales_return_reversal_intent','create'),('sales_return_reversal_intent','approve'),('purchase_return_reversal_intent','create'),('purchase_return_reversal_intent','approve')) AS capability(resource,action)
ON CONFLICT(capability) DO NOTHING;
-- No automatic grants or approval policies.
