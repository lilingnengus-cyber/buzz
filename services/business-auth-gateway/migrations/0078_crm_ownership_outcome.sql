-- Preserve existing ownership and history; no invented close dates or loss reasons.
ALTER TABLE crm_opportunities ADD COLUMN expected_close_date date;
ALTER TABLE crm_opportunities ADD COLUMN loss_reason text NOT NULL DEFAULT '' CHECK(char_length(loss_reason)<=1000);
ALTER TABLE crm_followups ADD COLUMN loss_reason text NOT NULL DEFAULT '' CHECK(char_length(loss_reason)<=1000);
CREATE INDEX crm_opportunities_owner_due ON crm_opportunities(owner_user_id,next_follow_up,id);
CREATE OR REPLACE VIEW crm_opportunity_current AS
SELECT o.id,o.legal_entity_id,o.business_unit_id,o.customer_id,o.title,
 COALESCE(b.name,a.name,o.company_name) AS company_name,
 COALESCE(c.name,o.contact_name) AS contact_name,COALESCE(c.details,o.contact_details) AS contact_details,
 o.stage,o.expected_amount_minor,o.currency,o.next_action,o.next_follow_up,o.owner_user_id,o.created_at,o.updated_at,o.version,o.account_id,o.contact_id,o.expected_close_date,o.loss_reason,u.display_name AS owner_name
FROM crm_opportunities o LEFT JOIN crm_accounts a ON a.id=o.account_id
LEFT JOIN business_customers b ON b.id=a.customer_id LEFT JOIN crm_contacts c ON c.id=o.contact_id JOIN enterprise_users u ON u.id=o.owner_user_id;
