CREATE TABLE life_workspace_memberships (
    id uuid PRIMARY KEY,
    workbench_user_id uuid NOT NULL REFERENCES life_workbench_users(id),
    workspace_id text NOT NULL CHECK (length(workspace_id) BETWEEN 1 AND 512),
    role_code text NOT NULL CHECK (length(role_code) BETWEEN 1 AND 128),
    status text NOT NULL CHECK (status IN ('active', 'revoked')),
    membership_version bigint NOT NULL CHECK (membership_version > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz
);

CREATE UNIQUE INDEX life_workspace_membership_active
    ON life_workspace_memberships(workbench_user_id, workspace_id)
    WHERE status = 'active';

CREATE TABLE life_principals (
    id uuid PRIMARY KEY,
    workbench_user_id uuid REFERENCES life_workbench_users(id),
    agent_id text,
    kind text NOT NULL CHECK (kind IN ('human', 'independent_agent')),
    status text NOT NULL CHECK (status IN ('active', 'disabled')),
    version bigint NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    disabled_at timestamptz,
    CHECK (
        (kind = 'human' AND workbench_user_id IS NOT NULL AND agent_id IS NULL)
        OR
        (kind = 'independent_agent' AND agent_id IS NOT NULL AND length(agent_id) BETWEEN 1 AND 512)
    )
);

CREATE UNIQUE INDEX life_principal_active_agent
    ON life_principals(agent_id)
    WHERE status = 'active' AND kind = 'independent_agent';

CREATE UNIQUE INDEX life_principal_active_human
    ON life_principals(workbench_user_id)
    WHERE status = 'active' AND kind = 'human';

CREATE TABLE life_capability_catalog (
    capability text NOT NULL CHECK (capability ~ '^[a-z0-9_.-]+:[a-z0-9_.:-]+$'),
    allowed_tools jsonb NOT NULL CHECK (jsonb_typeof(allowed_tools) = 'array'),
    risk_class text NOT NULL CHECK (risk_class IN ('low', 'medium', 'high')),
    requires_expected_version boolean NOT NULL,
    default_max_calls integer NOT NULL CHECK (default_max_calls BETWEEN 1 AND 1000),
    max_batch_size integer NOT NULL CHECK (max_batch_size BETWEEN 1 AND 10000),
    obligations jsonb NOT NULL CHECK (jsonb_typeof(obligations) = 'array'),
    catalog_version integer NOT NULL CHECK (catalog_version > 0),
    status text NOT NULL CHECK (status IN ('active', 'retired')),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (capability, catalog_version)
);

CREATE UNIQUE INDEX life_capability_catalog_active
    ON life_capability_catalog(capability)
    WHERE status = 'active';

CREATE TABLE life_principal_capabilities (
    principal_id uuid NOT NULL REFERENCES life_principals(id),
    capability text NOT NULL,
    catalog_version integer NOT NULL,
    data_scope jsonb NOT NULL CHECK (jsonb_typeof(data_scope) = 'object'),
    obligations jsonb NOT NULL CHECK (jsonb_typeof(obligations) = 'array'),
    status text NOT NULL CHECK (status IN ('active', 'revoked')),
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz,
    PRIMARY KEY (principal_id, capability, catalog_version),
    FOREIGN KEY (capability, catalog_version)
        REFERENCES life_capability_catalog(capability, catalog_version)
);

CREATE TABLE life_principal_data_scopes (
    principal_id uuid NOT NULL,
    capability text NOT NULL,
    catalog_version integer NOT NULL,
    dimension text NOT NULL CHECK (
        dimension IN ('workspace', 'domain', 'project', 'resource', 'sensitivity', 'operation_count')
    ),
    allowed_values jsonb NOT NULL CHECK (jsonb_typeof(allowed_values) = 'array'),
    status text NOT NULL CHECK (status IN ('active', 'revoked')),
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz,
    PRIMARY KEY (principal_id, capability, catalog_version, dimension),
    FOREIGN KEY (principal_id, capability, catalog_version)
        REFERENCES life_principal_capabilities(principal_id, capability, catalog_version)
);

CREATE TABLE life_iam_decisions (
    id uuid PRIMARY KEY,
    principal_id uuid REFERENCES life_principals(id),
    workbench_user_id uuid REFERENCES life_workbench_users(id),
    agent_id text NOT NULL CHECK (length(agent_id) BETWEEN 1 AND 512),
    agent_turn_id text NOT NULL CHECK (length(agent_turn_id) BETWEEN 1 AND 512),
    source_event_id text CHECK (source_event_id ~ '^[0-9a-f]{64}$'),
    requested_capabilities jsonb NOT NULL CHECK (jsonb_typeof(requested_capabilities) = 'array'),
    effective_grants jsonb NOT NULL CHECK (jsonb_typeof(effective_grants) = 'object'),
    denied_capabilities jsonb NOT NULL CHECK (jsonb_typeof(denied_capabilities) = 'array'),
    decision_reason text NOT NULL CHECK (length(decision_reason) BETWEEN 1 AND 128),
    catalog_version integer NOT NULL CHECK (catalog_version > 0),
    trace_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX life_iam_decisions_trace ON life_iam_decisions(trace_id, created_at);
