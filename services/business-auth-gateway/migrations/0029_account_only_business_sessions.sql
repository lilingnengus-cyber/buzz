-- Business Dock authorization is account-scoped. Existing Buzz identity
-- bindings remain available for agent delegation and historical audit, but
-- they no longer gate interactive Workbench or Business sessions.
ALTER TABLE embed_sessions
  ALTER COLUMN identity_binding_id DROP NOT NULL;

ALTER TABLE business_sessions
  ALTER COLUMN identity_binding_id DROP NOT NULL;

COMMENT ON COLUMN embed_sessions.identity_binding_id IS
  'Legacy optional Buzz identity binding retained for historical audit; interactive issuance is account-scoped.';

COMMENT ON COLUMN business_sessions.identity_binding_id IS
  'Legacy optional Buzz identity binding retained for historical audit; interactive sessions are account-scoped.';
