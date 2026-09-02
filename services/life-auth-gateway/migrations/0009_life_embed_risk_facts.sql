ALTER TABLE life_embed_codes
    ADD COLUMN issue_ip_hash bytea,
    ADD COLUMN issue_user_agent_hash bytea,
    ADD CONSTRAINT life_embed_codes_issue_ip_hash_len
        CHECK (issue_ip_hash IS NULL OR octet_length(issue_ip_hash) = 32),
    ADD CONSTRAINT life_embed_codes_issue_user_agent_hash_len
        CHECK (issue_user_agent_hash IS NULL OR octet_length(issue_user_agent_hash) = 32);

ALTER TABLE life_embed_sessions
    ADD COLUMN consume_ip_hash bytea,
    ADD COLUMN consume_user_agent_hash bytea,
    ADD CONSTRAINT life_embed_sessions_consume_ip_hash_len
        CHECK (consume_ip_hash IS NULL OR octet_length(consume_ip_hash) = 32),
    ADD CONSTRAINT life_embed_sessions_consume_user_agent_hash_len
        CHECK (consume_user_agent_hash IS NULL OR octet_length(consume_user_agent_hash) = 32);
