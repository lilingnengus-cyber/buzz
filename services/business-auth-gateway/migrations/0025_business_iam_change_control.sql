INSERT INTO business_iam.permissions(
  id,capability,resource_type,action,obligations,risk_level)
VALUES
  (gen_random_uuid(),'business_iam:read','business_iam','read','["step_up_authentication"]'::jsonb,'medium'),
  (gen_random_uuid(),'business_iam:request','business_iam','request','["step_up_authentication"]'::jsonb,'high'),
  (gen_random_uuid(),'business_iam:approve','business_iam','approve',
   '["step_up_authentication","dual_control"]'::jsonb,'critical')
ON CONFLICT(capability) DO UPDATE SET
  obligations=EXCLUDED.obligations,risk_level=EXCLUDED.risk_level,
  status='active',updated_at=now(),version=business_iam.permissions.version+1;

CREATE TABLE business_iam.change_requests (
  id uuid PRIMARY KEY,
  operation text NOT NULL CHECK (operation IN (
    'principal_upsert','principal_disable','role_upsert','role_disable',
    'permission_grant','permission_revoke','role_permission_grant',
    'role_permission_revoke','role_assign','role_unassign')),
  payload jsonb NOT NULL CHECK (jsonb_typeof(payload) = 'object'),
  payload_hash bytea NOT NULL CHECK (octet_length(payload_hash) = 32),
  risk_level text NOT NULL CHECK (risk_level IN ('high','critical')),
  required_approvals smallint NOT NULL CHECK (required_approvals IN (1,2)),
  status text NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending','approved','rejected','applied','failed','cancelled')),
  requested_by uuid NOT NULL REFERENCES business_iam.principals(id),
  requester_issuer text NOT NULL CHECK (length(requester_issuer) BETWEEN 1 AND 2048),
  requester_subject text NOT NULL CHECK (length(requester_subject) BETWEEN 1 AND 512),
  reason text NOT NULL CHECK (length(reason) BETWEEN 3 AND 500),
  idempotency_key text NOT NULL CHECK (length(idempotency_key) BETWEEN 8 AND 128),
  trace_id uuid NOT NULL,
  requested_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz NOT NULL DEFAULT (now() + interval '24 hours'),
  decided_at timestamptz,
  applied_at timestamptz,
  failure_code text,
  version bigint NOT NULL DEFAULT 1 CHECK (version > 0),
  UNIQUE (requested_by,idempotency_key),
  CHECK ((status IN ('approved','rejected','applied','failed')) = (decided_at IS NOT NULL)),
  CHECK ((status = 'applied') = (applied_at IS NOT NULL)),
  CHECK (failure_code IS NULL OR status = 'failed'),
  CHECK (expires_at > requested_at AND expires_at <= requested_at + interval '24 hours')
);

CREATE INDEX business_iam_change_requests_status_time_idx
  ON business_iam.change_requests(status,requested_at DESC);
CREATE INDEX business_iam_change_requests_requester_time_idx
  ON business_iam.change_requests(requested_by,requested_at DESC);

CREATE TABLE business_iam.change_approvals (
  id uuid PRIMARY KEY,
  change_request_id uuid NOT NULL REFERENCES business_iam.change_requests(id),
  approver_id uuid NOT NULL REFERENCES business_iam.principals(id),
  decision text NOT NULL CHECK (decision IN ('approve','reject')),
  comment text CHECK (comment IS NULL OR length(comment) BETWEEN 3 AND 500),
  step_up_at timestamptz NOT NULL,
  evidence_hash bytea NOT NULL CHECK (octet_length(evidence_hash) = 32),
  trace_id uuid NOT NULL,
  decided_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(change_request_id,approver_id)
);

CREATE INDEX business_iam_change_approvals_request_idx
  ON business_iam.change_approvals(change_request_id,decided_at);

CREATE TABLE business_iam.admin_audit_events (
  id uuid PRIMARY KEY,
  event_type text NOT NULL CHECK (length(event_type) BETWEEN 3 AND 100),
  result text NOT NULL CHECK (result IN ('success','denied','failed')),
  reason_code text,
  actor_principal_id uuid REFERENCES business_iam.principals(id),
  actor_issuer text,
  actor_subject text,
  change_request_id uuid REFERENCES business_iam.change_requests(id),
  trace_id uuid NOT NULL,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(metadata) = 'object'),
  database_actor text NOT NULL DEFAULT current_user,
  occurred_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX business_iam_admin_audit_trace_idx
  ON business_iam.admin_audit_events(trace_id);
CREATE INDEX business_iam_admin_audit_change_idx
  ON business_iam.admin_audit_events(change_request_id,occurred_at);

CREATE OR REPLACE FUNCTION business_iam.protect_change_request_identity()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF OLD.id IS DISTINCT FROM NEW.id
    OR OLD.operation IS DISTINCT FROM NEW.operation
    OR OLD.payload IS DISTINCT FROM NEW.payload
    OR OLD.payload_hash IS DISTINCT FROM NEW.payload_hash
    OR OLD.risk_level IS DISTINCT FROM NEW.risk_level
    OR OLD.required_approvals IS DISTINCT FROM NEW.required_approvals
    OR OLD.requested_by IS DISTINCT FROM NEW.requested_by
    OR OLD.requester_issuer IS DISTINCT FROM NEW.requester_issuer
    OR OLD.requester_subject IS DISTINCT FROM NEW.requester_subject
    OR OLD.reason IS DISTINCT FROM NEW.reason
    OR OLD.idempotency_key IS DISTINCT FROM NEW.idempotency_key
    OR OLD.trace_id IS DISTINCT FROM NEW.trace_id
    OR OLD.requested_at IS DISTINCT FROM NEW.requested_at
    OR OLD.expires_at IS DISTINCT FROM NEW.expires_at
  THEN
    RAISE EXCEPTION 'business_iam change request identity is immutable';
  END IF;
  RETURN NEW;
END $$;

CREATE TRIGGER business_iam_change_request_identity_immutable
  BEFORE UPDATE ON business_iam.change_requests
  FOR EACH ROW EXECUTE FUNCTION business_iam.protect_change_request_identity();

CREATE OR REPLACE FUNCTION business_iam.deny_admin_evidence_mutation()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'business_iam administrative evidence is append-only'; END $$;

CREATE TRIGGER business_iam_change_approvals_no_update_delete
  BEFORE UPDATE OR DELETE ON business_iam.change_approvals
  FOR EACH ROW EXECUTE FUNCTION business_iam.deny_admin_evidence_mutation();
CREATE TRIGGER business_iam_admin_audit_no_update_delete
  BEFORE UPDATE OR DELETE ON business_iam.admin_audit_events
  FOR EACH ROW EXECUTE FUNCTION business_iam.deny_admin_evidence_mutation();
