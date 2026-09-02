ALTER TABLE life_workbench_users
    ADD COLUMN authority_version bigint NOT NULL DEFAULT 0 CHECK (authority_version >= 0),
    ADD COLUMN authority_sync_status text NOT NULL DEFAULT 'stale'
        CHECK (authority_sync_status IN ('current', 'stale')),
    ADD COLUMN authority_synced_at timestamptz;

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

INSERT INTO life_capability_catalog
    (capability,allowed_tools,risk_class,requires_expected_version,
     default_max_calls,max_batch_size,obligations,catalog_version,status)
VALUES
('workspace:read','["get_system_overview"]','low',false,100,100,'[]',1,'active'),
('domain:read','[]','low',false,100,100,'[]',1,'active'),
('domain:create','[]','medium',false,25,25,'[]',1,'active'),
('domain:update','[]','medium',true,25,25,'[]',1,'active'),
('goal:read','[]','low',false,100,100,'[]',1,'active'),
('goal:create','["create_goal"]','medium',false,25,25,'[]',1,'active'),
('goal:update','[]','medium',true,25,25,'[]',1,'active'),
('goal:archive','[]','high',true,5,10,'["human_confirmation","step_up_authentication"]',1,'active'),
('project:read','["list_projects","get_project_context"]','low',false,100,100,'[]',1,'active'),
('project:create','["create_project"]','medium',false,25,25,'[]',1,'active'),
('project:update','[]','medium',true,25,25,'[]',1,'active'),
('project:archive','[]','high',true,5,10,'["human_confirmation","step_up_authentication"]',1,'active'),
('action:read','["list_actions","get_action_detail"]','low',false,100,100,'[]',1,'active'),
('action:create','["create_action"]','medium',false,25,25,'[]',1,'active'),
('action:update','["update_action"]','medium',true,25,25,'[]',1,'active'),
('action:status_update','["update_action_status"]','medium',true,25,25,'[]',1,'active'),
('action:reorder','["reorder_action_children"]','medium',true,25,25,'[]',1,'active'),
('action:delete','[]','high',true,5,10,'["human_confirmation","step_up_authentication"]',1,'active'),
('focus:read','["get_today_context"]','low',false,100,100,'[]',1,'active'),
('focus:update','[]','medium',true,25,25,'[]',1,'active'),
('focus:replace','["set_today_focus"]','medium',true,25,25,'[]',1,'active'),
('calendar:read','[]','low',false,100,100,'[]',1,'active'),
('calendar:create','[]','medium',false,25,25,'[]',1,'active'),
('calendar:update','[]','medium',true,25,25,'[]',1,'active'),
('calendar:delete','[]','high',true,5,10,'["human_confirmation","step_up_authentication"]',1,'active'),
('calendar:invite','[]','high',true,5,10,'["human_confirmation","step_up_authentication"]',1,'active'),
('journal:read','["search_journal"]','low',false,100,100,'[]',1,'active'),
('journal:create','["create_journal_entry"]','medium',false,25,25,'[]',1,'active'),
('journal:update','[]','medium',true,25,25,'[]',1,'active'),
('journal:delete','[]','high',true,5,10,'["human_confirmation","step_up_authentication"]',1,'active'),
('knowledge:read','["search_knowledge","get_knowledge_item"]','low',false,100,100,'[]',1,'active'),
('knowledge:create','["create_knowledge_item"]','medium',false,25,25,'[]',1,'active'),
('knowledge:update','[]','medium',true,25,25,'[]',1,'active'),
('knowledge:delete','[]','high',true,5,10,'["human_confirmation","step_up_authentication"]',1,'active'),
('knowledge:export','[]','high',false,5,10,'["human_confirmation","step_up_authentication"]',1,'active'),
('review:read','["get_review_context","get_weekly_review_context"]','low',false,100,100,'[]',1,'active'),
('review:create','["create_daily_review","create_project_review"]','medium',false,25,25,'[]',1,'active'),
('review:update','["apply_weekly_review"]','medium',true,25,25,'[]',1,'active'),
('ai_execution:read','["get_ai_execution_context"]','low',false,100,100,'[]',1,'active'),
('ai_execution:start','["start_ai_execution"]','medium',false,25,25,'[]',1,'active'),
('ai_execution:append_output','["append_ai_execution_output"]','medium',true,25,25,'[]',1,'active'),
('ai_execution:finish','["finish_ai_execution"]','medium',true,25,25,'[]',1,'active'),
('ai_execution:policy_update','[]','high',true,5,10,'["human_confirmation","step_up_authentication"]',1,'active'),
('notification:read','[]','low',false,100,100,'[]',1,'active'),
('notification:acknowledge','[]','medium',true,25,25,'[]',1,'active');

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

CREATE FUNCTION life_reject_iam_decision_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'life_iam_decisions is append-only' USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER life_iam_decisions_no_update
    BEFORE UPDATE ON life_iam_decisions
    FOR EACH ROW EXECUTE FUNCTION life_reject_iam_decision_mutation();

CREATE TRIGGER life_iam_decisions_no_delete
    BEFORE DELETE ON life_iam_decisions
    FOR EACH ROW EXECUTE FUNCTION life_reject_iam_decision_mutation();

REVOKE UPDATE, DELETE, TRUNCATE ON life_iam_decisions FROM PUBLIC;
