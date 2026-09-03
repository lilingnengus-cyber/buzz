CREATE TABLE life_pacioli_target_selections (
    id uuid PRIMARY KEY,
    kind text NOT NULL CHECK (kind IN ('identity', 'channel')),
    workbench_user_id uuid NOT NULL REFERENCES life_workbench_users(id),
    life_os_user_id text NOT NULL CHECK (length(life_os_user_id) BETWEEN 1 AND 512),
    community_id text NOT NULL CHECK (length(community_id) BETWEEN 1 AND 256),
    user_pubkey text NOT NULL CHECK (user_pubkey ~ '^[0-9a-f]{64}$'),
    channel_id text CHECK (channel_id IS NULL OR length(channel_id) BETWEEN 1 AND 256),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    trace_id uuid NOT NULL,
    CHECK (expires_at > created_at),
    CHECK ((kind = 'identity' AND channel_id IS NULL) OR
           (kind = 'channel' AND channel_id IS NOT NULL))
);

CREATE INDEX life_pacioli_target_selections_expiry
    ON life_pacioli_target_selections(expires_at)
    WHERE consumed_at IS NULL;
