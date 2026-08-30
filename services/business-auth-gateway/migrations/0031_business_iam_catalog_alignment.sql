-- Keep the management-plane catalog aligned with the read capabilities
-- enforced by Business Core.
INSERT INTO business_iam.permissions(
  id,capability,resource_type,action,risk_level
)
VALUES (
  gen_random_uuid(),'management_report:read','management_report','read','low'
)
ON CONFLICT (capability) DO UPDATE SET
  resource_type=EXCLUDED.resource_type,
  action=EXCLUDED.action,
  status='active',
  updated_at=now(),
  version=business_iam.permissions.version+1;

-- Production no longer uses MFA step-up or dual-control obligations. Keep
-- human approval where a capability explicitly requires it.
UPDATE business_iam.permissions
SET obligations=(obligations - 'step_up_authentication' - 'dual_control'),
    updated_at=now(),
    version=version+1
WHERE obligations ?| ARRAY['step_up_authentication','dual_control'];
