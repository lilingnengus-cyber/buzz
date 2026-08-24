-- Proxy agents are execution context, not durable IAM principals. Historical
-- proxy principals and decisions remain readable for audit continuity.

ALTER TABLE business_iam.authorization_decisions
  ALTER COLUMN agent_principal_id DROP NOT NULL,
  ALTER COLUMN agent_kind DROP NOT NULL,
  ALTER COLUMN policy_version SET DEFAULT 'business-iam-v2';

ALTER TABLE business_iam.authorization_decisions
  ADD COLUMN executor_type text,
  ADD COLUMN executor_id text;

-- The v1 table is append-only. This migration performs the only permitted
-- deterministic backfill while the surrounding SQLx migration transaction is
-- holding the schema change.
ALTER TABLE business_iam.authorization_decisions
  DISABLE TRIGGER business_iam_decisions_no_update_delete;

UPDATE business_iam.authorization_decisions decision
SET executor_type = CASE
      WHEN decision.agent_kind = 'independent_agent' THEN 'independent_agent'
      ELSE 'proxy_agent'
    END,
    executor_id = principal.external_id
FROM business_iam.principals principal
WHERE principal.id = decision.agent_principal_id;

UPDATE business_iam.authorization_decisions decision
SET executor_type = COALESCE(decision.executor_type, 'proxy_agent'),
    executor_id = COALESCE(decision.executor_id, delegation.agent_id, 'legacy-unknown')
FROM agent_read_delegations delegation
WHERE delegation.iam_decision_id = decision.id
  AND (decision.executor_type IS NULL OR decision.executor_id IS NULL);

UPDATE business_iam.authorization_decisions
SET executor_type = COALESCE(executor_type, 'proxy_agent'),
    executor_id = COALESCE(executor_id, 'legacy-unknown');

ALTER TABLE business_iam.authorization_decisions
  ENABLE TRIGGER business_iam_decisions_no_update_delete;

ALTER TABLE business_iam.authorization_decisions
  ALTER COLUMN executor_type SET NOT NULL,
  ALTER COLUMN executor_id SET NOT NULL,
  ADD CONSTRAINT business_iam_decisions_executor_type_check
    CHECK (executor_type IN ('proxy_agent','independent_agent')),
  ADD CONSTRAINT business_iam_decisions_executor_id_check
    CHECK (length(executor_id) BETWEEN 1 AND 200),
  ADD CONSTRAINT business_iam_decisions_executor_authority_check
    CHECK (
      (executor_type = 'proxy_agent'
        AND human_principal_id IS NOT NULL
        AND agent_principal_id IS NULL
        AND agent_kind IS NULL)
      OR
      (executor_type = 'independent_agent'
        AND agent_principal_id IS NOT NULL
        AND agent_kind = 'independent_agent')
      OR policy_version = 'business-iam-v1'
    );

CREATE INDEX business_iam_decisions_executor_time_idx
  ON business_iam.authorization_decisions (executor_type, executor_id, decided_at DESC);

-- Retire old proxy principals without deleting immutable decision history.
UPDATE business_iam.principals
SET status = 'disabled', disabled_at = COALESCE(disabled_at, now()),
    updated_at = now(), version = version + 1,
    metadata = metadata || '{"legacyProxyPrincipal":true}'::jsonb
WHERE kind = 'proxy_agent' AND status = 'active';

DELETE FROM business_iam.principal_roles
WHERE principal_id IN (
  SELECT id FROM business_iam.principals WHERE kind = 'proxy_agent'
);

DELETE FROM business_iam.principal_permissions
WHERE principal_id IN (
  SELECT id FROM business_iam.principals WHERE kind = 'proxy_agent'
);

CREATE OR REPLACE FUNCTION business_iam.reject_proxy_principal_write()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF NEW.kind = 'proxy_agent' THEN
    RAISE EXCEPTION 'proxy_agent is an execution context, not an IAM principal';
  END IF;
  RETURN NEW;
END $$;

CREATE TRIGGER business_iam_proxy_principal_writes_rejected
  BEFORE INSERT OR UPDATE OF kind ON business_iam.principals
  FOR EACH ROW EXECUTE FUNCTION business_iam.reject_proxy_principal_write();

COMMENT ON COLUMN business_iam.authorization_decisions.executor_type IS
  'Business executor class. proxy_agent has no durable IAM principal.';
COMMENT ON COLUMN business_iam.authorization_decisions.executor_id IS
  'Technical executor instance identity used for binding and audit, not authorization.';
