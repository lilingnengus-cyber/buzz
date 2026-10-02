-- Service classification is permanent, so historical documents retain their
-- inventory semantics even after master data is edited.
ALTER TABLE business_products ADD COLUMN service_kind text NOT NULL DEFAULT 'goods'
 CHECK(service_kind IN ('goods','technical_service','software_service'));
CREATE FUNCTION preserve_business_product_kind() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.service_kind IS DISTINCT FROM OLD.service_kind THEN
  RAISE EXCEPTION '商品类型建立后不可修改，请新建商品';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER preserve_business_product_kind BEFORE UPDATE ON business_products
 FOR EACH ROW EXECUTE FUNCTION preserve_business_product_kind();

CREATE TABLE service_projects (
 id uuid PRIMARY KEY,
 title text NOT NULL CHECK(length(trim(title)) BETWEEN 1 AND 200),
 legal_entity_id uuid NOT NULL REFERENCES business_legal_entities(id),
 business_unit_id uuid NOT NULL REFERENCES business_units(id),
 customer_id uuid NOT NULL REFERENCES business_customers(id),
 owner_user_id uuid NOT NULL REFERENCES enterprise_users(id),
 contact_name text NOT NULL DEFAULT '',
 service_kind text NOT NULL CHECK(service_kind IN ('technical_service','software_service')),
 sales_order_line_id uuid UNIQUE REFERENCES sales_order_lines(id),
 renewal_of_project_id uuid REFERENCES service_projects(id),
 starts_on date,
 ends_on date,
 status text NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','active','acceptance','completed','paused','cancelled')),
 description text NOT NULL DEFAULT '',
 version bigint NOT NULL DEFAULT 1,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 CHECK(ends_on IS NULL OR starts_on IS NULL OR ends_on>=starts_on),
 CHECK(renewal_of_project_id IS DISTINCT FROM id)
);
CREATE INDEX service_projects_scope ON service_projects(legal_entity_id,business_unit_id,customer_id);
CREATE TABLE service_deliverables (
 id uuid PRIMARY KEY,
 project_id uuid NOT NULL REFERENCES service_projects(id),
 title text NOT NULL CHECK(length(trim(title)) BETWEEN 1 AND 200),
 owner_user_id uuid NOT NULL REFERENCES enterprise_users(id),
 due_on date,
 status text NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','active','completed','cancelled')),
 description text NOT NULL DEFAULT '',
 evidence_url text NOT NULL DEFAULT '',
 version bigint NOT NULL DEFAULT 1,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX service_deliverables_project ON service_deliverables(project_id,created_at,id);
CREATE TABLE service_acceptances (
 id uuid PRIMARY KEY,
 project_id uuid NOT NULL REFERENCES service_projects(id),
 accepted_on date NOT NULL,
 customer_reviewer text NOT NULL CHECK(length(trim(customer_reviewer)) BETWEEN 1 AND 200),
 result text NOT NULL CHECK(result IN ('passed','rejected')),
 note text NOT NULL DEFAULT '',
 evidence_url text NOT NULL DEFAULT '',
 actor_user_id uuid NOT NULL REFERENCES enterprise_users(id),
 created_at timestamptz NOT NULL DEFAULT now(),
 trace_id uuid NOT NULL
);
CREATE INDEX service_acceptances_project ON service_acceptances(project_id,created_at,id);
INSERT INTO business_role_permissions(role_id,permission_key)
SELECT id,permission FROM business_roles
CROSS JOIN (VALUES ('service_delivery:read'),('service_delivery:manage')) p(permission)
WHERE role_key IN ('business_admin','s1_operator') ON CONFLICT DO NOTHING;
INSERT INTO business_iam.permissions(id,capability,resource_type,action,risk_level)
VALUES(gen_random_uuid(),'service_delivery:read','service_delivery','read','low'),
(gen_random_uuid(),'service_delivery:manage','service_delivery','manage','medium') ON CONFLICT(capability) DO NOTHING;
CREATE TRIGGER service_acceptances_immutable BEFORE UPDATE OR DELETE ON service_acceptances
 FOR EACH ROW EXECUTE FUNCTION deny_business_document_approval_vote_mutation();
