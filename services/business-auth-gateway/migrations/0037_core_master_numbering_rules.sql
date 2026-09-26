-- Core master data joins the governed numbering ledger. Codes remain immutable
-- after creation, while the configured rule controls every new allocation.

ALTER TABLE business_numbering_rules
    DROP CONSTRAINT business_numbering_rules_record_type_check;
ALTER TABLE business_numbering_rules
    ADD CONSTRAINT business_numbering_rules_record_type_check CHECK (record_type IN (
        'legal_entity','business_unit','customer','supplier','warehouse',
        'sales_order','shipment','receivable','receipt','opening',
        'purchase_order','goods_receipt','payable','supplier_payment',
        'sales_return','purchase_return','inventory_count',
        'purchase_requisition','profit_adjustment','management_report'
    ));

CREATE SEQUENCE business_legal_entity_number_seq;
CREATE SEQUENCE business_business_unit_number_seq;
CREATE SEQUENCE business_customer_number_seq;
CREATE SEQUENCE business_supplier_number_seq;
CREATE SEQUENCE business_warehouse_number_seq;

INSERT INTO business_numbering_rules(id,record_type,name,segments,reset_period,scope_dimension)
VALUES
('20000000-0000-0000-0000-000000000016','legal_entity','法定主体编码','[{"type":"fixed","value":"LE-"},{"type":"sequence","width":4}]','never','global'),
('20000000-0000-0000-0000-000000000017','business_unit','经营主体编码','[{"type":"fixed","value":"OU-"},{"type":"sequence","width":4}]','never','global'),
('20000000-0000-0000-0000-000000000018','customer','客户编码','[{"type":"fixed","value":"CU-"},{"type":"sequence","width":5}]','never','legal_entity'),
('20000000-0000-0000-0000-000000000019','supplier','供应商编码','[{"type":"fixed","value":"SU-"},{"type":"sequence","width":5}]','never','legal_entity'),
('20000000-0000-0000-0000-000000000020','warehouse','仓库编码','[{"type":"fixed","value":"WH-"},{"type":"sequence","width":4}]','never','legal_entity')
ON CONFLICT (record_type) DO NOTHING;
