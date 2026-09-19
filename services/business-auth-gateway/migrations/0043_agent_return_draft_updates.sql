INSERT INTO business_iam.permissions(id,capability,resource_type,action,obligations,risk_level)
SELECT gen_random_uuid(),resource||':update_draft',resource,'update_draft','[]'::jsonb,'high'
FROM (VALUES ('sales_return'),('purchase_return')) AS capability(resource)
ON CONFLICT(capability) DO NOTHING;
-- No automatic grants. Existing draft modification cannot confirm or cancel a return.
