CREATE TABLE crm_leads (
 id uuid PRIMARY KEY,
 title text NOT NULL CHECK(length(trim(title)) BETWEEN 1 AND 160),
 company_name text NOT NULL DEFAULT '', contact_name text NOT NULL DEFAULT '',
 contact_details text NOT NULL DEFAULT '', source text NOT NULL DEFAULT '',
 summary text NOT NULL DEFAULT '', next_action text NOT NULL DEFAULT '', next_follow_up date,
 status text NOT NULL DEFAULT 'new' CHECK(status IN ('new','contacting','converted','disqualified')),
 disqualification_reason text NOT NULL DEFAULT '',
 customer_id uuid REFERENCES business_customers(id),
 owner_user_id uuid NOT NULL REFERENCES enterprise_users(id),
 created_by_user_id uuid NOT NULL REFERENCES enterprise_users(id),
 converted_opportunity_id uuid UNIQUE REFERENCES crm_opportunities(id),
 version bigint NOT NULL DEFAULT 1, created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
 CHECK((status='converted')=(converted_opportunity_id IS NOT NULL)),
 CHECK(status!='disqualified' OR length(trim(disqualification_reason))>0)
);
CREATE INDEX crm_lead_owner_due ON crm_leads(owner_user_id,status,next_follow_up,id);
CREATE TABLE crm_lead_followups (
 id uuid PRIMARY KEY, lead_id uuid NOT NULL REFERENCES crm_leads(id),
 author_user_id uuid NOT NULL REFERENCES enterprise_users(id),
 note text NOT NULL CHECK(length(trim(note)) BETWEEN 1 AND 4000),
 disqualification_reason text NOT NULL DEFAULT '',
 status text NOT NULL, next_action text NOT NULL, next_follow_up date,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX crm_lead_followup_history ON crm_lead_followups(lead_id,created_at DESC,id);

ALTER TABLE crm_followups ADD COLUMN source_lead_id uuid REFERENCES crm_leads(id);
