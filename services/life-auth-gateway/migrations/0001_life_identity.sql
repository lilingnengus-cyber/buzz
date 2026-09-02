CREATE TABLE life_workbench_users (
    id uuid PRIMARY KEY,
    oidc_issuer text NOT NULL CHECK (length(oidc_issuer) BETWEEN 1 AND 512),
    oidc_subject text NOT NULL CHECK (length(oidc_subject) BETWEEN 1 AND 512),
    life_os_user_id text NOT NULL CHECK (length(life_os_user_id) BETWEEN 1 AND 512),
    status text NOT NULL CHECK (status IN ('active', 'disabled')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    disabled_at timestamptz,
    UNIQUE (oidc_issuer, oidc_subject)
);

CREATE TABLE life_identity_binding_challenges (
    id uuid PRIMARY KEY,
    workbench_user_id uuid NOT NULL REFERENCES life_workbench_users(id),
    buzz_pubkey text NOT NULL CHECK (buzz_pubkey ~ '^[0-9a-f]{64}$'),
    nonce_hash bytea NOT NULL CHECK (octet_length(nonce_hash) = 32),
    status text NOT NULL CHECK (status IN ('active', 'consumed', 'expired', 'revoked')),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    revoked_at timestamptz,
    CHECK (expires_at > created_at)
);

CREATE UNIQUE INDEX life_binding_challenge_one_active
    ON life_identity_binding_challenges(workbench_user_id, buzz_pubkey)
    WHERE status = 'active';

CREATE TABLE life_identity_bindings (
    id uuid PRIMARY KEY,
    workbench_user_id uuid NOT NULL REFERENCES life_workbench_users(id),
    buzz_pubkey text NOT NULL CHECK (buzz_pubkey ~ '^[0-9a-f]{64}$'),
    status text NOT NULL CHECK (status IN ('active', 'revoked')),
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz,
    version bigint NOT NULL DEFAULT 1 CHECK (version > 0)
);

CREATE UNIQUE INDEX life_identity_binding_active_pubkey
    ON life_identity_bindings(buzz_pubkey)
    WHERE status = 'active';

CREATE TABLE life_workbench_sessions (
    id uuid PRIMARY KEY,
    workbench_user_id uuid NOT NULL REFERENCES life_workbench_users(id),
    deployment_id text NOT NULL CHECK (length(deployment_id) BETWEEN 1 AND 256),
    token_hash bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    oidc_session_id text,
    status text NOT NULL CHECK (status IN ('active', 'revoked', 'expired')),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    CHECK (expires_at > created_at)
);

CREATE INDEX life_workbench_sessions_active_user
    ON life_workbench_sessions(workbench_user_id, deployment_id)
    WHERE status = 'active';
