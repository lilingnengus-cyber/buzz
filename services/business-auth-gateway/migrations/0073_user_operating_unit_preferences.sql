CREATE TABLE business_user_operating_unit_preferences (
    enterprise_user_id UUID NOT NULL
        REFERENCES enterprise_users(id) ON DELETE CASCADE,
    context TEXT NOT NULL
        CHECK (context ~ '^[a-z][a-z0-9:_-]{0,63}$'),
    business_unit_id UUID NOT NULL
        REFERENCES business_units(id) ON DELETE CASCADE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (enterprise_user_id, context)
);

CREATE INDEX business_user_operating_unit_preferences_unit_idx
    ON business_user_operating_unit_preferences (business_unit_id);
