ALTER TABLE life_embed_codes
    ADD COLUMN identity_binding_id uuid REFERENCES life_identity_bindings(id);

CREATE INDEX life_embed_codes_active_binding
    ON life_embed_codes(identity_binding_id, expires_at)
    WHERE status = 'active' AND identity_binding_id IS NOT NULL;
