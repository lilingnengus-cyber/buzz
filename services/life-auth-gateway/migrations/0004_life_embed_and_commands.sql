CREATE TABLE life_embed_codes (
    id uuid PRIMARY KEY,
    code_hash bytea NOT NULL UNIQUE CHECK (octet_length(code_hash) = 32),
    workbench_user_id uuid NOT NULL REFERENCES life_workbench_users(id),
    workbench_session_id uuid NOT NULL REFERENCES life_workbench_sessions(id),
    deployment_id text NOT NULL CHECK (length(deployment_id) BETWEEN 1 AND 256),
    target_path text NOT NULL CHECK (target_path LIKE '/%' AND length(target_path) BETWEEN 1 AND 2048),
    status text NOT NULL CHECK (status IN ('active', 'consumed', 'revoked', 'expired')),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    revoked_at timestamptz,
    trace_id uuid NOT NULL,
    CHECK (expires_at > created_at)
);

CREATE INDEX life_embed_codes_active_user
    ON life_embed_codes(workbench_user_id, expires_at)
    WHERE status = 'active';

CREATE TABLE life_embed_sessions (
    id uuid PRIMARY KEY,
    session_token_hash bytea NOT NULL UNIQUE CHECK (octet_length(session_token_hash) = 32),
    workbench_user_id uuid NOT NULL REFERENCES life_workbench_users(id),
    workbench_session_id uuid NOT NULL REFERENCES life_workbench_sessions(id),
    deployment_id text NOT NULL CHECK (length(deployment_id) BETWEEN 1 AND 256),
    status text NOT NULL CHECK (status IN ('active', 'revoked', 'expired')),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    trace_id uuid NOT NULL,
    CHECK (expires_at > created_at)
);

CREATE INDEX life_embed_sessions_active_user
    ON life_embed_sessions(workbench_user_id, deployment_id, expires_at)
    WHERE status = 'active';

CREATE TABLE life_write_command_confirmations (
    id uuid PRIMARY KEY,
    command_id uuid NOT NULL,
    workbench_user_id uuid NOT NULL REFERENCES life_workbench_users(id),
    workbench_session_id uuid NOT NULL REFERENCES life_workbench_sessions(id),
    source_event_id text NOT NULL CHECK (source_event_id ~ '^[0-9a-f]{64}$'),
    expected_version bigint NOT NULL CHECK (expected_version >= 0),
    preview_hash bytea NOT NULL CHECK (octet_length(preview_hash) = 32),
    status text NOT NULL CHECK (status IN ('active', 'consumed', 'revoked', 'expired')),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    trace_id uuid NOT NULL,
    CHECK (expires_at > created_at)
);

CREATE UNIQUE INDEX life_write_confirmation_active_command
    ON life_write_command_confirmations(command_id)
    WHERE status = 'active';
