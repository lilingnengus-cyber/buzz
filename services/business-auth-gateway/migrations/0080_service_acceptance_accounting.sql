ALTER TABLE sales_order_lines ALTER COLUMN warehouse_id DROP NOT NULL;
ALTER TABLE sales_order_lines ADD COLUMN service_kind text NOT NULL DEFAULT 'goods'
 CHECK(service_kind IN ('goods','technical_service','software_service'));
ALTER TABLE sales_order_lines ADD COLUMN service_fulfilled_quantity numeric(24,6) NOT NULL DEFAULT 0 CHECK(service_fulfilled_quantity>=0);
ALTER TABLE sales_order_lines ADD CONSTRAINT sales_line_service_quantities CHECK(
 (service_kind='goods' AND warehouse_id IS NOT NULL AND service_fulfilled_quantity=0)
 OR (service_kind<>'goods' AND warehouse_id IS NULL AND reserved_quantity=0 AND shipped_quantity=0 AND service_fulfilled_quantity+cancelled_quantity<=ordered_quantity));
CREATE FUNCTION snapshot_sales_line_service_kind() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='INSERT' OR NEW.sku_id IS DISTINCT FROM OLD.sku_id THEN
  SELECT p.service_kind INTO NEW.service_kind FROM business_skus s JOIN business_products p ON p.id=s.product_id WHERE s.id=NEW.sku_id;
 ELSIF NEW.service_kind IS DISTINCT FROM OLD.service_kind THEN
  RAISE EXCEPTION '销售行类型不可更改';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER snapshot_sales_line_service_kind BEFORE INSERT OR UPDATE ON sales_order_lines
 FOR EACH ROW EXECUTE FUNCTION snapshot_sales_line_service_kind();
ALTER TABLE sales_orders DROP CONSTRAINT sales_orders_fulfillment_status_check;
ALTER TABLE sales_orders ADD CONSTRAINT sales_orders_fulfillment_status_check CHECK(fulfillment_status IN ('unreserved','reserved','partially_shipped','shipped','cancelled','service_pending','partially_fulfilled','fulfilled'));
ALTER TABLE trade_receivables ALTER COLUMN shipment_id DROP NOT NULL;
ALTER TABLE trade_receivables ADD COLUMN service_project_id uuid UNIQUE REFERENCES service_projects(id);
ALTER TABLE trade_receivables ADD COLUMN service_acceptance_id uuid UNIQUE REFERENCES service_acceptances(id);
ALTER TABLE trade_receivables ADD CONSTRAINT receivable_single_delivery_source CHECK(
 (shipment_id IS NOT NULL AND service_project_id IS NULL AND service_acceptance_id IS NULL)
 OR (shipment_id IS NULL AND service_project_id IS NOT NULL AND service_acceptance_id IS NOT NULL));
ALTER TABLE profit_facts DROP CONSTRAINT profit_facts_source_type_check;
ALTER TABLE profit_facts ADD CONSTRAINT profit_facts_source_type_check CHECK(source_type IN ('shipment','operational_adjustment','sales_return','service_acceptance'));
INSERT INTO business_role_permissions(role_id,permission_key)
 SELECT id,'service_delivery:accept' FROM business_roles WHERE role_key IN ('business_admin','s1_operator') ON CONFLICT DO NOTHING;
INSERT INTO business_iam.permissions(id,capability,resource_type,action,risk_level)
 VALUES(gen_random_uuid(),'service_delivery:accept','service_delivery','accept','high') ON CONFLICT(capability) DO NOTHING;
-- Service SKUs can never be used to fabricate physical inventory movements.
CREATE FUNCTION require_physical_inventory_sku() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM business_skus s JOIN business_products p ON p.id=s.product_id WHERE s.id=NEW.sku_id AND p.service_kind<>'goods') THEN
  RAISE EXCEPTION '服务商品不能发生库存操作';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER require_physical_inventory_sku BEFORE INSERT OR UPDATE ON inventory_balances
 FOR EACH ROW EXECUTE FUNCTION require_physical_inventory_sku();
CREATE TRIGGER require_physical_reservation_sku BEFORE INSERT OR UPDATE ON inventory_reservations
 FOR EACH ROW EXECUTE FUNCTION require_physical_inventory_sku();
