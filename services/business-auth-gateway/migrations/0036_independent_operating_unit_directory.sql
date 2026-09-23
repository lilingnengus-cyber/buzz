-- Remove the last read-model dependency on the retained compatibility owner.
-- The column itself remains until the later contract release.
CREATE OR REPLACE VIEW business_master_data_directory AS
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
UNION ALL SELECT 'warehouse',id,code,name,status,legal_entity_id,id,NULL,NULL,NULL,business_unit_id,version FROM business_warehouses
UNION ALL SELECT 'customer',id,code,name,status,legal_entity_id,NULL,id,NULL,NULL,business_unit_id,version FROM business_customers
UNION ALL SELECT 'supplier',id,code,name,status,legal_entity_id,NULL,NULL,id,NULL,business_unit_id,version FROM business_suppliers
UNION ALL SELECT 'product',p.id,p.code,p.name,p.status,NULL,NULL,NULL,NULL,p.brand_id,NULL,p.version FROM business_products p
UNION ALL SELECT 'sku',s.id,s.code,s.name,s.status,NULL,NULL,NULL,NULL,p.brand_id,NULL,s.version FROM business_skus s JOIN business_products p ON p.id=s.product_id
UNION ALL SELECT 'salesperson',id,code,name,status,NULL,NULL,NULL,NULL,NULL,business_unit_id,version FROM business_salespeople;
