ALTER TABLE life_identity_binding_challenges
    ADD COLUMN workbench_session_id uuid NOT NULL REFERENCES life_workbench_sessions(id),
    ADD COLUMN deployment_id text NOT NULL CHECK (length(deployment_id) BETWEEN 1 AND 256);

CREATE INDEX life_binding_challenge_active_session
    ON life_identity_binding_challenges(workbench_session_id, expires_at)
    WHERE status = 'active';

ALTER TABLE life_identity_bindings
    ADD COLUMN source_event_id text NOT NULL UNIQUE
        CHECK (source_event_id ~ '^[0-9a-f]{64}$');

ALTER TABLE life_agent_delegations
    ADD COLUMN identity_binding_id uuid REFERENCES life_identity_bindings(id);

ALTER TABLE life_embed_sessions
    ADD COLUMN identity_binding_id uuid REFERENCES life_identity_bindings(id);

ALTER TABLE life_security_audit
    ADD COLUMN identity_binding_id uuid REFERENCES life_identity_bindings(id);

CREATE INDEX life_agent_delegations_binding
    ON life_agent_delegations(identity_binding_id)
    WHERE identity_binding_id IS NOT NULL AND status IN ('active', 'exhausted');

CREATE INDEX life_embed_sessions_binding
    ON life_embed_sessions(identity_binding_id)
    WHERE identity_binding_id IS NOT NULL AND status = 'active';
