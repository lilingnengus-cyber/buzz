DROP INDEX life_write_confirmation_active_command;

CREATE UNIQUE INDEX life_write_confirmation_command_once
    ON life_write_command_confirmations(command_id);

ALTER TABLE life_agent_delegations
    ADD COLUMN iam_decision_id uuid REFERENCES life_iam_decisions(id),
    ADD COLUMN source_channel_id text CHECK (
        source_channel_id IS NULL OR length(source_channel_id) BETWEEN 1 AND 512
    ),
    ADD COLUMN conversation_context jsonb NOT NULL DEFAULT '{}'::jsonb
        CHECK (jsonb_typeof(conversation_context) = 'object'),
    ADD COLUMN resource_context jsonb CHECK (
        resource_context IS NULL OR jsonb_typeof(resource_context) = 'object'
    ),
    ADD COLUMN write_command_id uuid REFERENCES life_write_command_confirmations(command_id),
    ADD COLUMN catalog_version integer NOT NULL DEFAULT 1 CHECK (catalog_version > 0),
    ADD COLUMN version bigint NOT NULL DEFAULT 1 CHECK (version > 0),
    ADD COLUMN last_used_at timestamptz;

CREATE UNIQUE INDEX life_agent_delegation_source_event_once
    ON life_agent_delegations(source_event_id);
