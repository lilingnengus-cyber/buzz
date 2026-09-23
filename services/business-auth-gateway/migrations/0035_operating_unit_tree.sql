-- Decouple the operating hierarchy from legal ownership while preserving the
-- legacy legal_entity_id column for one compatibility release.

ALTER TABLE business_units
    ADD COLUMN parent_business_unit_id UUID
        REFERENCES business_units(id) ON DELETE RESTRICT,
    ADD COLUMN is_operating_root BOOLEAN NOT NULL DEFAULT FALSE,
    ADD CONSTRAINT business_units_not_own_parent
        CHECK (parent_business_unit_id IS NULL OR parent_business_unit_id <> id),
    ADD CONSTRAINT business_units_root_has_no_parent
        CHECK (NOT is_operating_root OR parent_business_unit_id IS NULL);

CREATE UNIQUE INDEX business_units_one_root
    ON business_units (is_operating_root) WHERE is_operating_root;
CREATE INDEX business_units_parent_idx
    ON business_units (parent_business_unit_id);

DO $$
DECLARE
    unit_count BIGINT;
    root_id UUID;
    compatibility_legal_entity_id UUID;
    group_name TEXT;
BEGIN
    SELECT count(*) INTO unit_count FROM business_units;
    IF unit_count = 0 THEN
        RETURN;
    END IF;

    IF unit_count = 1 THEN
        UPDATE business_units
        SET is_operating_root = TRUE,
            parent_business_unit_id = NULL,
            updated_at = now(),
            version = version + 1;
        RETURN;
    END IF;

    SELECT id INTO compatibility_legal_entity_id
    FROM business_legal_entities
    ORDER BY code
    LIMIT 1;
    SELECT name INTO group_name FROM business_group_profile WHERE singleton;
    root_id := gen_random_uuid();
    INSERT INTO business_units(
        id,legal_entity_id,code,name,is_operating_root
    ) VALUES (
        root_id,
        compatibility_legal_entity_id,
        'GROUP_OPERATIONS',
        COALESCE(group_name,'集团经营主体'),
        TRUE
    );
    UPDATE business_units
    SET parent_business_unit_id = root_id,
        updated_at = now(),
        version = version + 1
    WHERE id <> root_id;
END $$;

DROP VIEW core_master_data_maintenance;

CREATE VIEW core_master_data_maintenance AS
WITH RECURSIVE unit_tree AS (
    SELECT
        unit.id,
        unit.parent_business_unit_id,
        ARRAY[unit.name]::TEXT[] AS path,
        0::INTEGER AS depth
    FROM business_units unit
    WHERE unit.parent_business_unit_id IS NULL
    UNION ALL
    SELECT
        child.id,
        child.parent_business_unit_id,
        parent.path || child.name,
        parent.depth + 1
    FROM business_units child
    JOIN unit_tree parent ON child.parent_business_unit_id = parent.id
),
unit_closure AS (
    SELECT unit.id AS ancestor_id, unit.id AS descendant_id
    FROM business_units unit
    UNION ALL
    SELECT closure.ancestor_id, child.id
    FROM unit_closure closure
    JOIN business_units child
      ON child.parent_business_unit_id = closure.descendant_id
),
unit_directory AS (
    SELECT
        unit.id,
        unit.parent_business_unit_id,
        tree.path,
        tree.depth,
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
       directory.parent_business_unit_id,directory.path,directory.depth,
       directory.descendant_count
FROM business_units u
JOIN unit_directory directory ON directory.id=u.id
UNION ALL
SELECT 'customer',c.id,c.code,c.name,c.status,
       e.id,e.code,e.name,u.id,u.code,u.name,
       NULL,NULL,NULL,NULL,c.credit_currency::TEXT,c.credit_limit_minor,c.payment_terms_days,
       c.version,c.updated_at,
       directory.parent_business_unit_id,directory.path,directory.depth,
       directory.descendant_count
FROM business_customers c
JOIN business_legal_entities e ON e.id=c.legal_entity_id
JOIN business_units u ON u.id=c.business_unit_id
JOIN unit_directory directory ON directory.id=u.id
UNION ALL
SELECT 'supplier',s.id,s.code,s.name,s.status,
       e.id,e.code,e.name,u.id,u.code,u.name,
       NULL,NULL,NULL,NULL,NULL,NULL,s.payment_terms_days,s.version,s.updated_at,
       directory.parent_business_unit_id,directory.path,directory.depth,
       directory.descendant_count
FROM business_suppliers s
JOIN business_legal_entities e ON e.id=s.legal_entity_id
JOIN business_units u ON u.id=s.business_unit_id
JOIN unit_directory directory ON directory.id=u.id
UNION ALL
SELECT 'warehouse',w.id,w.code,w.name,w.status,
       e.id,e.code,e.name,u.id,u.code,u.name,
       NULL,NULL,NULL,w.address,NULL,NULL,NULL,w.version,w.updated_at,
       directory.parent_business_unit_id,directory.path,directory.depth,
       directory.descendant_count
FROM business_warehouses w
JOIN business_legal_entities e ON e.id=w.legal_entity_id
JOIN business_units u ON u.id=w.business_unit_id
JOIN unit_directory directory ON directory.id=u.id;
