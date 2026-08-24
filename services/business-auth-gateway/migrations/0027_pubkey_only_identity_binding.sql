-- Enterprise access is bound to the portable Buzz identity, not a device.
-- Device fields remain nullable so historical audit rows and old bindings keep
-- their provenance without participating in authorization.

DROP INDEX IF EXISTS buzz_identity_bindings_active_device;

ALTER TABLE buzz_identity_bindings
  ALTER COLUMN device_id DROP NOT NULL,
  ALTER COLUMN device_name DROP NOT NULL,
  ALTER COLUMN device_platform DROP NOT NULL;

UPDATE buzz_identity_bindings
SET device_id = NULL,
    device_name = NULL,
    device_platform = NULL,
    updated_at = now(),
    version = version + 1
WHERE device_id IS NOT NULL
   OR device_name IS NOT NULL
   OR device_platform IS NOT NULL;

UPDATE identity_binding_challenges
SET status = 'revoked'
WHERE status = 'active';

ALTER TABLE identity_binding_challenges
  ALTER COLUMN device_id DROP NOT NULL,
  ALTER COLUMN device_name DROP NOT NULL,
  ALTER COLUMN device_platform DROP NOT NULL,
  DROP CONSTRAINT IF EXISTS identity_binding_challenges_audience_check;

ALTER TABLE identity_binding_challenges
  ADD CONSTRAINT identity_binding_challenges_audience_check
  CHECK (audience IN (
    'bizfin-workbench-device-binding',
    'bizfin-workbench-identity-binding'
  ));

COMMENT ON COLUMN buzz_identity_bindings.device_id IS
  'Legacy audit metadata only; never used for authorization.';
COMMENT ON COLUMN buzz_identity_bindings.device_name IS
  'Legacy audit metadata only; never used for authorization.';
COMMENT ON COLUMN buzz_identity_bindings.device_platform IS
  'Legacy audit metadata only; never used for authorization.';
