-- Business IAM keeps an explicit approval event, but no longer requires
-- dual control or a reviewer distinct from the requester.
DROP TRIGGER business_iam_change_request_identity_immutable
  ON business_iam.change_requests;

UPDATE business_iam.change_requests
SET required_approvals = 1
WHERE status = 'pending' AND required_approvals <> 1;

CREATE TRIGGER business_iam_change_request_identity_immutable
  BEFORE UPDATE ON business_iam.change_requests
  FOR EACH ROW EXECUTE FUNCTION business_iam.protect_change_request_identity();

UPDATE business_iam.permissions
SET obligations = (obligations - 'step_up_authentication' - 'dual_control'),
    updated_at = now(),
    version = version + 1
WHERE capability IN (
  'business_iam:read',
  'business_iam:request',
  'business_iam:approve'
)
AND obligations ?| ARRAY['step_up_authentication','dual_control'];
