-- Customers, suppliers, and warehouses are group-shared master data. Legal
-- entity and operating-unit dimensions belong to business documents, not to
-- these reusable directory records. Retain the nullable columns for one
-- compatibility release while making the read and write contract unbound.

DROP VIEW core_master_data_maintenance;
DROP VIEW business_master_data_directory;

ALTER TABLE business_customers
    ALTER COLUMN legal_entity_id DROP NOT NULL,
    ALTER COLUMN business_unit_id DROP NOT NULL;
ALTER TABLE business_suppliers
    ALTER COLUMN legal_entity_id DROP NOT NULL,
    ALTER COLUMN business_unit_id DROP NOT NULL;
ALTER TABLE business_warehouses
    ALTER COLUMN legal_entity_id DROP NOT NULL,
    ALTER COLUMN business_unit_id DROP NOT NULL;

UPDATE business_customers
SET legal_entity_id = NULL, business_unit_id = NULL, version = version + 1, updated_at = now()
WHERE legal_entity_id IS NOT NULL OR business_unit_id IS NOT NULL;
UPDATE business_suppliers
SET legal_entity_id = NULL, business_unit_id = NULL, version = version + 1, updated_at = now()
WHERE legal_entity_id IS NOT NULL OR business_unit_id IS NOT NULL;
UPDATE business_warehouses
SET legal_entity_id = NULL, business_unit_id = NULL, version = version + 1, updated_at = now()
WHERE legal_entity_id IS NOT NULL OR business_unit_id IS NOT NULL;

UPDATE business_numbering_rules
SET segments = CASE record_type
        WHEN 'customer' THEN '[{"type":"fixed","value":"CU-"},{"type":"sequence","width":6}]'::jsonb
        WHEN 'supplier' THEN '[{"type":"fixed","value":"SU-"},{"type":"sequence","width":6}]'::jsonb
        ELSE '[{"type":"fixed","value":"WH-"},{"type":"sequence","width":6}]'::jsonb
    END,
    scope_dimension = 'global',
    version = version + 1,
    updated_at = now()
WHERE record_type IN ('customer', 'supplier', 'warehouse');

CREATE VIEW business_master_data_directory AS
SELECT 'legal_entity'::TEXT resource_type,id,code,name,status,id legal_entity_id,
       NULL::UUID warehouse_id,NULL::UUID customer_id,NULL::UUID supplier_id,
       NULL::UUID brand_id,NULL::UUID business_unit_id,version
FROM business_legal_entities
UNION ALL SELECT 'ledger_book',id,code,name,status,legal_entity_id,NULL,NULL,NULL,NULL,NULL,version FROM business_ledger_books
UNION ALL SELECT 'business_unit',id,code,name,status,NULL,NULL,NULL,NULL,NULL,id,version FROM business_units
UNION ALL SELECT 'department',id,code,name,status,NULL,NULL,NULL,NULL,NULL,business_unit_id,version FROM business_departments
UNION ALL SELECT 'unit_of_measure',id,code,name,status,NULL,NULL,NULL,NULL,NULL,NULL,version FROM business_units_of_measure
UNION ALL SELECT 'product_category',id,code,name,status,NULL,NULL,NULL,NULL,NULL,NULL,version FROM business_product_categories
UNION ALL SELECT 'brand',id,code,name,status,NULL,NULL,NULL,NULL,id,NULL,version FROM business_brands
UNION ALL SELECT 'warehouse',id,code,name,status,NULL,id,NULL,NULL,NULL,NULL,version FROM business_warehouses
UNION ALL SELECT 'customer',id,code,name,status,NULL,NULL,id,NULL,NULL,NULL,version FROM business_customers
UNION ALL SELECT 'supplier',id,code,name,status,NULL,NULL,NULL,id,NULL,NULL,version FROM business_suppliers
UNION ALL SELECT 'product',p.id,p.code,p.name,p.status,NULL,NULL,NULL,NULL,p.brand_id,NULL,p.version FROM business_products p
UNION ALL SELECT 'sku',s.id,s.code,s.name,s.status,NULL,NULL,NULL,NULL,p.brand_id,NULL,s.version FROM business_skus s JOIN business_products p ON p.id=s.product_id
UNION ALL SELECT 'salesperson',id,code,name,status,NULL,NULL,NULL,NULL,NULL,business_unit_id,version FROM business_salespeople;

CREATE VIEW core_master_data_maintenance AS
WITH RECURSIVE unit_tree AS (
    SELECT unit.id, unit.parent_business_unit_id, ARRAY[unit.name]::TEXT[] AS path, 0::INTEGER AS depth
    FROM business_units unit WHERE unit.parent_business_unit_id IS NULL
    UNION ALL
    SELECT child.id, child.parent_business_unit_id, parent.path || child.name, parent.depth + 1
    FROM business_units child JOIN unit_tree parent ON child.parent_business_unit_id = parent.id
),
unit_closure AS (
    SELECT unit.id AS ancestor_id, unit.id AS descendant_id FROM business_units unit
    UNION ALL
    SELECT closure.ancestor_id, child.id FROM unit_closure closure
    JOIN business_units child ON child.parent_business_unit_id = closure.descendant_id
),
unit_directory AS (
    SELECT unit.id, unit.parent_business_unit_id, tree.path, tree.depth,
           count(closure.descendant_id) - 1 AS descendant_count
    FROM business_units unit
    JOIN unit_tree tree ON tree.id = unit.id
    JOIN unit_closure closure ON closure.ancestor_id = unit.id
    GROUP BY unit.id,unit.parent_business_unit_id,tree.path,tree.depth
)
SELECT 'legal_entity'::TEXT resource_type,e.id,e.code,e.name,e.status,
       e.id legal_entity_id,e.code legal_entity_code,e.name legal_entity_name,
       NULL::UUID business_unit_id,NULL::TEXT business_unit_code,NULL::TEXT business_unit_name,
       e.country_code::TEXT,e.functional_currency::TEXT,e.registration_number,
       NULL::TEXT address,NULL::TEXT credit_currency,NULL::BIGINT credit_limit_minor,
       NULL::INTEGER payment_terms_days,e.version,e.updated_at,
       NULL::UUID parent_business_unit_id,NULL::TEXT[] business_unit_path,
       NULL::INTEGER business_unit_depth,NULL::BIGINT descendant_count
FROM business_legal_entities e
UNION ALL
SELECT 'business_unit',u.id,u.code,u.name,u.status,
       NULL,NULL,NULL,u.id,u.code,u.name,
       NULL,NULL,NULL,NULL,NULL,NULL,NULL,u.version,u.updated_at,
       directory.parent_business_unit_id,directory.path,directory.depth,directory.descendant_count
FROM business_units u JOIN unit_directory directory ON directory.id=u.id
UNION ALL
SELECT 'customer',c.id,c.code,c.name,c.status,
       NULL,NULL,NULL,NULL,NULL,NULL,
       NULL,NULL,NULL,NULL,c.credit_currency::TEXT,c.credit_limit_minor,c.payment_terms_days,
       c.version,c.updated_at,NULL,NULL,NULL,NULL
FROM business_customers c
UNION ALL
SELECT 'supplier',s.id,s.code,s.name,s.status,
       NULL,NULL,NULL,NULL,NULL,NULL,
       NULL,NULL,NULL,NULL,NULL,NULL,s.payment_terms_days,s.version,s.updated_at,
       NULL,NULL,NULL,NULL
FROM business_suppliers s
UNION ALL
SELECT 'warehouse',w.id,w.code,w.name,w.status,
       NULL,NULL,NULL,NULL,NULL,NULL,
       NULL,NULL,NULL,w.address,NULL,NULL,NULL,w.version,w.updated_at,
       NULL,NULL,NULL,NULL
FROM business_warehouses w;
