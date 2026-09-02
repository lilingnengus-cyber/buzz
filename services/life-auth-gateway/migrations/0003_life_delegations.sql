CREATE TABLE life_agent_delegations (
    id uuid PRIMARY KEY,
    token_hash bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    workbench_user_id uuid NOT NULL REFERENCES life_workbench_users(id),
    workbench_session_id uuid NOT NULL REFERENCES life_workbench_sessions(id),
    principal_id uuid REFERENCES life_principals(id),
    agent_id text NOT NULL CHECK (length(agent_id) BETWEEN 1 AND 512),
    agent_turn_id text NOT NULL CHECK (length(agent_turn_id) BETWEEN 1 AND 512),
    source_event_id text NOT NULL CHECK (source_event_id ~ '^[0-9a-f]{64}$'),
    source_pubkey text NOT NULL CHECK (source_pubkey ~ '^[0-9a-f]{64}$'),
    audience text NOT NULL CHECK (audience = 'life-workbench-mcp'),
    capabilities jsonb NOT NULL CHECK (jsonb_typeof(capabilities) = 'array'),
    data_scope jsonb NOT NULL CHECK (jsonb_typeof(data_scope) = 'object'),
    obligations jsonb NOT NULL CHECK (jsonb_typeof(obligations) = 'array'),
    status text NOT NULL CHECK (status IN ('active', 'exhausted', 'revoked', 'expired')),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    max_calls integer NOT NULL CHECK (max_calls BETWEEN 1 AND 1000),
    remaining_calls integer NOT NULL CHECK (remaining_calls BETWEEN 0 AND max_calls),
    trace_id uuid NOT NULL,
    CHECK (expires_at > created_at)
);

CREATE UNIQUE INDEX life_agent_delegation_active_turn
    ON life_agent_delegations(agent_turn_id, source_event_id, audience)
    WHERE status = 'active';

CREATE INDEX life_agent_delegation_active_user
    ON life_agent_delegations(workbench_user_id, expires_at)
    WHERE status = 'active';

CREATE TABLE life_delegation_calls (
    id uuid PRIMARY KEY,
    delegation_id uuid NOT NULL REFERENCES life_agent_delegations(id),
    call_id uuid NOT NULL,
    capability text NOT NULL CHECK (length(capability) BETWEEN 3 AND 128),
    normalized_input_hash bytea NOT NULL CHECK (octet_length(normalized_input_hash) = 32),
    idempotency_key text NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    resource_type text,
    resource_id text,
    expected_version bigint,
    status text NOT NULL CHECK (status IN ('issued', 'succeeded', 'failed')),
    trace_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    UNIQUE (delegation_id, call_id),
    UNIQUE (delegation_id, idempotency_key)
);

CREATE TABLE life_security_audit (
    id bigserial PRIMARY KEY,
    event_type text NOT NULL CHECK (length(event_type) BETWEEN 1 AND 128),
    outcome text NOT NULL CHECK (outcome IN ('success', 'failure', 'denied')),
    reason_code text,
    subject_kind text,
    subject_id text,
    workbench_user_id uuid,
    workbench_session_id uuid,
    delegation_id uuid,
    source_event_id text CHECK (source_event_id ~ '^[0-9a-f]{64}$'),
    resource_type text,
    resource_id_hash bytea CHECK (resource_id_hash IS NULL OR octet_length(resource_id_hash) = 32),
    metadata_hash bytea CHECK (metadata_hash IS NULL OR octet_length(metadata_hash) = 32),
    trace_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX life_security_audit_trace ON life_security_audit(trace_id, created_at);

CREATE FUNCTION life_reject_security_audit_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'life_security_audit is append-only' USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER life_security_audit_no_update
    BEFORE UPDATE ON life_security_audit
    FOR EACH ROW EXECUTE FUNCTION life_reject_security_audit_mutation();

CREATE TRIGGER life_security_audit_no_delete
    BEFORE DELETE ON life_security_audit
    FOR EACH ROW EXECUTE FUNCTION life_reject_security_audit_mutation();

REVOKE UPDATE, DELETE, TRUNCATE ON life_security_audit FROM PUBLIC;
