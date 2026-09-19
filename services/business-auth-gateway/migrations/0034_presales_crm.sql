-- Small, durable presales pipeline; no order, inventory or accounting effects.
CREATE TABLE crm_opportunities (
 id uuid PRIMARY KEY,
 legal_entity_id uuid NOT NULL REFERENCES business_legal_entities(id),
 business_unit_id uuid NOT NULL REFERENCES business_units(id),
 customer_id uuid REFERENCES business_customers(id),
 title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 160),
 company_name text NOT NULL CHECK (char_length(company_name) BETWEEN 1 AND 160),
 contact_name text NOT NULL DEFAULT '' CHECK (char_length(contact_name)<=100),
 contact_details text NOT NULL DEFAULT '' CHECK (char_length(contact_details)<=200),
 stage text NOT NULL DEFAULT 'new' CHECK(stage IN ('new','contacting','quoting','won','lost')),
 expected_amount_minor bigint CHECK(expected_amount_minor BETWEEN 0 AND 999999999999),
 currency text NOT NULL DEFAULT 'CNY' CHECK(currency ~ '^[A-Z]{3}$'),
 next_action text NOT NULL DEFAULT '' CHECK(char_length(next_action)<=500),
 next_follow_up date,
 owner_user_id uuid NOT NULL REFERENCES enterprise_users(id),
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 version bigint NOT NULL DEFAULT 1 CHECK(version>0)
);
CREATE INDEX crm_opportunities_scope_due ON crm_opportunities(legal_entity_id,business_unit_id,next_follow_up,id);
CREATE TABLE crm_followups (
 id uuid PRIMARY KEY,
 opportunity_id uuid NOT NULL REFERENCES crm_opportunities(id),
 author_user_id uuid NOT NULL REFERENCES enterprise_users(id),
 note text NOT NULL CHECK(char_length(note) BETWEEN 1 AND 4000),
 stage text NOT NULL CHECK(stage IN ('new','contacting','quoting','won','lost')),
 next_action text NOT NULL CHECK(char_length(next_action)<=500),
 next_follow_up date,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX crm_followups_opportunity ON crm_followups(opportunity_id,created_at,id);
INSERT INTO business_role_permissions(role_id,permission_key)
SELECT id,permission FROM business_roles
CROSS JOIN (VALUES ('crm:read'),('crm:manage')) p(permission)
WHERE role_key IN ('business_admin','s1_operator') ON CONFLICT DO NOTHING;
INSERT INTO business_iam.permissions(id,capability,resource_type,action,risk_level)
VALUES(gen_random_uuid(),'crm:read','crm','read','low'),
(gen_random_uuid(),'crm:manage','crm','manage','medium') ON CONFLICT(capability) DO NOTHING;
