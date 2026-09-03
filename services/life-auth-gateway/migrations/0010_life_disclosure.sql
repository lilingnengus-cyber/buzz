ALTER TABLE life_agent_delegations
    ADD COLUMN disclosure_community_id text CHECK (
        disclosure_community_id IS NULL OR length(disclosure_community_id) BETWEEN 1 AND 512
    ),
    ADD COLUMN disclosure_policy_id uuid,
    ADD COLUMN disclosure_category text CHECK (
        disclosure_category IS NULL OR disclosure_category IN ('action_summary', 'project_status')
    ),
    ADD COLUMN disclosure_sensitivity text CHECK (
        disclosure_sensitivity IS NULL OR disclosure_sensitivity IN ('PUBLIC', 'NORMAL')
    ),
    ADD COLUMN disclosure_expires_at timestamptz,
    ADD CONSTRAINT life_agent_delegation_disclosure_complete CHECK (
        (disclosure_policy_id IS NULL AND disclosure_community_id IS NULL
            AND disclosure_category IS NULL AND disclosure_sensitivity IS NULL
            AND disclosure_expires_at IS NULL)
        OR
        (disclosure_policy_id IS NOT NULL AND disclosure_community_id IS NOT NULL
            AND disclosure_category IS NOT NULL AND disclosure_sensitivity IS NOT NULL
            AND disclosure_expires_at IS NOT NULL)
    );

CREATE INDEX life_agent_delegation_disclosure_expiry
    ON life_agent_delegations(disclosure_policy_id, disclosure_expires_at)
    WHERE disclosure_policy_id IS NOT NULL;
