CREATE TABLE life_pending_deletes (
    command_id uuid PRIMARY KEY,
    delegation_id uuid NOT NULL UNIQUE REFERENCES life_agent_delegations(id),
    community_id text NOT NULL,
    channel_id text NOT NULL,
    agent_id text NOT NULL,
    user_pubkey text NOT NULL,
    preview_event_id text NOT NULL UNIQUE,
    thread_root_id text NOT NULL,
    expected_version bigint NOT NULL CHECK (expected_version > 0),
    preview_hash text NOT NULL CHECK (preview_hash ~ '^[0-9a-f]{64}$'),
    shown_event_seconds bigint NOT NULL,
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX life_pending_deletes_scope ON life_pending_deletes
    (community_id, channel_id, agent_id, user_pubkey, created_at DESC);
