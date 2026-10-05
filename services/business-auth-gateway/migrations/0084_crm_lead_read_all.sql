-- Explicit read-only visibility across lead owners. Ordinary CRM operators retain
-- owner/creator isolation; this does not grant edit, follow-up or conversion rights.
INSERT INTO business_role_permissions(role_id,permission_key)
SELECT id,'crm:lead_read_all' FROM business_roles
WHERE role_key='business_admin' ON CONFLICT DO NOTHING;
INSERT INTO business_iam.permissions(id,capability,resource_type,action,risk_level)
VALUES(gen_random_uuid(),'crm:lead_read_all','crm','lead_read_all','medium')
ON CONFLICT(capability) DO NOTHING;
