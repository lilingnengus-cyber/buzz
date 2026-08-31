ALTER TABLE agent_read_delegations
  DROP CONSTRAINT agent_read_delegations_scopes_check,
  ADD CONSTRAINT agent_read_delegations_scopes_check
    CHECK (cardinality(scopes) BETWEEN 1 AND 16),
  ADD COLUMN approval_document_type text
    CHECK (approval_document_type IS NULL OR approval_document_type IN ('sales_order','purchase_order')),
  ADD COLUMN approval_document_id uuid,
  ADD COLUMN approval_expected_version bigint CHECK (approval_expected_version IS NULL OR approval_expected_version > 0),
  ADD COLUMN approval_preview_hash text
    CHECK (approval_preview_hash IS NULL OR approval_preview_hash ~ '^[0-9a-f]{64}$'),
  ADD COLUMN approval_decision text
    CHECK (approval_decision IS NULL OR approval_decision IN ('approve','reject')),
  ADD CONSTRAINT agent_delegations_approval_context_complete CHECK (
    (approval_document_type IS NULL AND approval_document_id IS NULL
      AND approval_expected_version IS NULL AND approval_preview_hash IS NULL
      AND approval_decision IS NULL)
    OR
    (approval_document_type IS NOT NULL AND approval_document_id IS NOT NULL
      AND approval_expected_version IS NOT NULL AND approval_preview_hash IS NOT NULL
      AND approval_decision IS NOT NULL)
  );

INSERT INTO business_iam.permissions(
  id,capability,resource_type,action,obligations,risk_level
)
VALUES
  (gen_random_uuid(),'sales_order:approve','sales_order','approve',
   '["fresh_signed_chat_command"]'::jsonb,'high'),
  (gen_random_uuid(),'purchase_order:approve','purchase_order','approve',
   '["fresh_signed_chat_command"]'::jsonb,'high');

CREATE TABLE business_document_approval_requests (
  id uuid PRIMARY KEY,
  document_type text NOT NULL CHECK (document_type IN ('sales_order','purchase_order')),
  document_id uuid NOT NULL,
  action_code text NOT NULL CHECK (action_code IN ('sales_order:confirm','purchase_order:confirm')),
  expected_version bigint NOT NULL CHECK (expected_version > 0),
  preview_hash text NOT NULL CHECK (preview_hash ~ '^[0-9a-f]{64}$'),
  requester_user_id uuid NOT NULL REFERENCES enterprise_users(id),
  minimum_approvers smallint NOT NULL CHECK (minimum_approvers BETWEEN 1 AND 10),
  status text NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending','approved','rejected','executing','executed','execution_failed','superseded')),
  created_at timestamptz NOT NULL DEFAULT now(),
  decided_at timestamptz,
  executed_at timestamptz,
  trace_id uuid NOT NULL,
  version bigint NOT NULL DEFAULT 1 CHECK (version > 0),
  UNIQUE (document_type, document_id, expected_version)
);

CREATE TABLE business_document_approval_votes (
  id uuid PRIMARY KEY,
  request_id uuid NOT NULL REFERENCES business_document_approval_requests(id),
  approver_user_id uuid NOT NULL REFERENCES enterprise_users(id),
  decision text NOT NULL CHECK (decision IN ('approve','reject')),
  source_buzz_event_id text NOT NULL CHECK (source_buzz_event_id ~ '^[0-9a-f]{64}$'),
  source_channel_id text NOT NULL CHECK (length(source_channel_id) BETWEEN 1 AND 200),
  trace_id uuid NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (request_id, approver_user_id),
  UNIQUE (source_buzz_event_id)
);

CREATE INDEX business_document_approval_pending_idx
  ON business_document_approval_requests(document_type, document_id)
  WHERE status IN ('pending','approved','executing','execution_failed');

CREATE OR REPLACE FUNCTION deny_business_document_approval_vote_mutation()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'business_document_approval_votes is append-only'; END $$;

CREATE TRIGGER business_document_approval_votes_no_update_delete
  BEFORE UPDATE OR DELETE ON business_document_approval_votes
  FOR EACH ROW EXECUTE FUNCTION deny_business_document_approval_vote_mutation();
