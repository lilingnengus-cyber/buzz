-- Register the fixed Agent capability used for customer creation. This only
-- adds it to the IAM catalog; administrators still grant it per account or
-- role with an explicit data scope.
INSERT INTO business_iam.permissions(
  id,capability,resource_type,action,risk_level
)
VALUES (
  gen_random_uuid(),'business_master_data:manage','business_master_data','manage','medium'
)
ON CONFLICT (capability) DO UPDATE SET
  resource_type=EXCLUDED.resource_type,
  action=EXCLUDED.action,
  risk_level=EXCLUDED.risk_level,
  status='active',
  updated_at=now(),
  version=business_iam.permissions.version+1;
