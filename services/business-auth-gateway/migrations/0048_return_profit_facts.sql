-- Permit the return facts already emitted by the projection, including corrections.
ALTER TABLE profit_facts DROP CONSTRAINT profit_facts_source_system_check,
 ADD CONSTRAINT profit_facts_source_system_check CHECK(source_system IN ('business_core_b2','business_core_b4','business_core_returns'));
ALTER TABLE profit_facts DROP CONSTRAINT profit_facts_source_type_check,
 ADD CONSTRAINT profit_facts_source_type_check CHECK(source_type IN ('shipment','operational_adjustment','sales_return'));
