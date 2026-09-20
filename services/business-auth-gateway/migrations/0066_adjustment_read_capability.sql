-- Register only; no user or agent gains authority automatically.
INSERT INTO business_iam.permissions(id,capability,resource_type,action,obligations,risk_level)
VALUES(gen_random_uuid(),'profit_adjustment:read','profit_adjustment','read','[]'::jsonb,'low')
ON CONFLICT(capability) DO NOTHING;
