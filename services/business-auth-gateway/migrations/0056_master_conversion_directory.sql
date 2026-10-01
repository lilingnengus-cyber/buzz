-- Preserve the directory shape; unit conversion ownership follows its product brand.
CREATE OR REPLACE VIEW business_master_data_directory AS
SELECT 'legal_entity'::TEXT resource_type, id, code, name, status, id legal_entity_id,
       NULL::UUID warehouse_id, NULL::UUID customer_id, NULL::UUID supplier_id,
       NULL::UUID brand_id, NULL::UUID business_unit_id, version
FROM business_legal_entities
UNION ALL SELECT 'ledger_book', id, code, name, status, legal_entity_id, NULL, NULL, NULL, NULL, NULL, version FROM business_ledger_books
UNION ALL SELECT 'business_unit', id, code, name, status, legal_entity_id, NULL, NULL, NULL, NULL, id, version FROM business_units
UNION ALL SELECT 'department', d.id, d.code, d.name, d.status, u.legal_entity_id, NULL, NULL, NULL, NULL, d.business_unit_id, d.version FROM business_departments d JOIN business_units u ON u.id=d.business_unit_id
UNION ALL SELECT 'unit_of_measure', id, code, name, status, NULL, NULL, NULL, NULL, NULL, NULL, version FROM business_units_of_measure
UNION ALL SELECT 'product_category', id, code, name, status, NULL, NULL, NULL, NULL, NULL, NULL, version FROM business_product_categories
UNION ALL SELECT 'brand', id, code, name, status, NULL, NULL, NULL, NULL, id, NULL, version FROM business_brands
UNION ALL SELECT 'warehouse', id, code, name, status, legal_entity_id, id, NULL, NULL, NULL, business_unit_id, version FROM business_warehouses
UNION ALL SELECT 'customer', id, code, name, status, legal_entity_id, NULL, id, NULL, NULL, business_unit_id, version FROM business_customers
UNION ALL SELECT 'supplier', id, code, name, status, legal_entity_id, NULL, NULL, id, NULL, business_unit_id, version FROM business_suppliers
UNION ALL SELECT 'product', p.id, p.code, p.name, p.status, NULL, NULL, NULL, NULL, p.brand_id, NULL, p.version FROM business_products p
UNION ALL SELECT 'sku', s.id, s.code, s.name, s.status, NULL, NULL, NULL, NULL, p.brand_id, NULL, s.version FROM business_skus s JOIN business_products p ON p.id=s.product_id
UNION ALL SELECT 'salesperson', s.id, s.code, s.name, s.status, u.legal_entity_id, NULL, NULL, NULL, NULL, s.business_unit_id, s.version FROM business_salespeople s JOIN business_units u ON u.id=s.business_unit_id
UNION ALL SELECT 'uom_conversion', x.id, p.code||':'||u.code, p.name||' / '||u.name,
       x.status, NULL, NULL, NULL, NULL, p.brand_id, NULL, x.version
FROM business_product_uom_conversions x
JOIN business_products p ON p.id=x.product_id
JOIN business_units_of_measure u ON u.id=x.unit_of_measure_id;
