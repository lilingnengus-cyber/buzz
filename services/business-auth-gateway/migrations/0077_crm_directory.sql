-- Independent presales directory; a linked customer always reuses core identity.
CREATE TABLE crm_accounts (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 customer_id uuid UNIQUE REFERENCES business_customers(id),
 name text NOT NULL CHECK(char_length(name) BETWEEN 1 AND 160),
 owner_user_id uuid NOT NULL REFERENCES enterprise_users(id),
 version bigint NOT NULL DEFAULT 1 CHECK(version > 0)
);
CREATE UNIQUE INDEX crm_prospect_name ON crm_accounts(owner_user_id, lower(btrim(name))) WHERE customer_id IS NULL;
CREATE TABLE crm_contacts (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 account_id uuid NOT NULL REFERENCES crm_accounts(id),
 name text NOT NULL CHECK(char_length(name) BETWEEN 1 AND 100),
 details text NOT NULL DEFAULT '' CHECK(char_length(details) <= 200),
 version bigint NOT NULL DEFAULT 1 CHECK(version > 0),
 UNIQUE(account_id, name, details),
 UNIQUE(id, account_id)
);
ALTER TABLE crm_opportunities ADD COLUMN account_id uuid REFERENCES crm_accounts(id);
ALTER TABLE crm_opportunities ADD COLUMN contact_id uuid;
ALTER TABLE crm_opportunities ADD CONSTRAINT crm_contact_account FOREIGN KEY(contact_id,account_id) REFERENCES crm_contacts(id,account_id);
INSERT INTO crm_accounts(customer_id,name,owner_user_id)
SELECT DISTINCT ON (o.customer_id) o.customer_id,c.name,o.owner_user_id
FROM crm_opportunities o JOIN business_customers c ON c.id=o.customer_id ORDER BY o.customer_id,o.created_at,o.id;
INSERT INTO crm_accounts(name,owner_user_id)
SELECT DISTINCT ON(owner_user_id,lower(btrim(company_name))) btrim(company_name),owner_user_id
FROM crm_opportunities WHERE customer_id IS NULL ORDER BY owner_user_id,lower(btrim(company_name)),created_at,id;
UPDATE crm_opportunities o SET account_id=a.id FROM crm_accounts a
WHERE (o.customer_id=a.customer_id) OR (o.customer_id IS NULL AND a.customer_id IS NULL AND o.owner_user_id=a.owner_user_id AND lower(btrim(o.company_name))=lower(a.name));
INSERT INTO crm_contacts(account_id,name,details)
SELECT DISTINCT account_id,btrim(contact_name),btrim(contact_details) FROM crm_opportunities WHERE btrim(contact_name)<>'';
UPDATE crm_opportunities o SET contact_id=c.id FROM crm_contacts c WHERE c.account_id=o.account_id AND c.name=btrim(o.contact_name) AND c.details=btrim(o.contact_details);
CREATE INDEX crm_opportunities_account ON crm_opportunities(account_id);
CREATE INDEX crm_opportunities_contact ON crm_opportunities(contact_id);
-- Preserve original text snapshots; reads display current directory information.
CREATE VIEW crm_opportunity_current AS
SELECT o.id,o.legal_entity_id,o.business_unit_id,o.customer_id,o.title,
 COALESCE(b.name,a.name,o.company_name) AS company_name,
 COALESCE(c.name,o.contact_name) AS contact_name,COALESCE(c.details,o.contact_details) AS contact_details,
 o.stage,o.expected_amount_minor,o.currency,o.next_action,o.next_follow_up,o.owner_user_id,o.created_at,o.updated_at,o.version,o.account_id,o.contact_id
FROM crm_opportunities o LEFT JOIN crm_accounts a ON a.id=o.account_id
LEFT JOIN business_customers b ON b.id=a.customer_id LEFT JOIN crm_contacts c ON c.id=o.contact_id;
