-- Separate global product reads from existing legal-entity-scoped master grants.
INSERT INTO business_iam.permissions(id,capability,resource_type,action,obligations,risk_level)
VALUES(gen_random_uuid(),'business_product_master:read','business_product_master','read','[]'::jsonb,'low')
ON CONFLICT(capability) DO NOTHING;
-- No automatic grants or changes to existing master-data read grants.
