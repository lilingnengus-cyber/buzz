-- Register lookup independently from transactional read/write capabilities.
-- This does not grant access: human and agent entitlements still intersect,
-- and Core enforces current master-data permission and record scopes.
INSERT INTO business_iam.permissions(id,capability,resource_type,action,risk_level)
VALUES (gen_random_uuid(),'business_master_data:read','business_master_data','read','low')
ON CONFLICT (capability) DO NOTHING;
