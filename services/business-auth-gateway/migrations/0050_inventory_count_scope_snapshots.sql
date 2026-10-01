-- Historical scope cannot be reconstructed from today's mutable master data.
ALTER TABLE inventory_count_tasks
    ADD COLUMN scope_snapshot_captured BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN snapshot_business_unit_id UUID REFERENCES business_units(id) ON DELETE RESTRICT,
    ADD CONSTRAINT inventory_count_scope_snapshot_complete CHECK (
        NOT scope_snapshot_captured OR snapshot_business_unit_id IS NOT NULL
    );
ALTER TABLE inventory_count_lines
    ADD COLUMN snapshot_brand_id UUID REFERENCES business_brands(id) ON DELETE RESTRICT;

CREATE FUNCTION preserve_inventory_count_scope_snapshot() RETURNS TRIGGER AS $$
BEGIN
    IF TG_TABLE_NAME = 'inventory_count_tasks' THEN
        IF (NEW.scope_snapshot_captured,NEW.snapshot_business_unit_id,NEW.legal_entity_id,NEW.warehouse_id)
            IS DISTINCT FROM
           (OLD.scope_snapshot_captured,OLD.snapshot_business_unit_id,OLD.legal_entity_id,OLD.warehouse_id) THEN
            RAISE EXCEPTION 'inventory count scope snapshot is immutable';
        END IF;
    ELSE
        IF (NEW.snapshot_brand_id,NEW.inventory_count_id,NEW.sku_id)
            IS DISTINCT FROM (OLD.snapshot_brand_id,OLD.inventory_count_id,OLD.sku_id) THEN
            RAISE EXCEPTION 'inventory count line scope snapshot is immutable';
        END IF;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER inventory_count_scope_snapshot_immutable BEFORE UPDATE ON inventory_count_tasks
    FOR EACH ROW EXECUTE FUNCTION preserve_inventory_count_scope_snapshot();
CREATE TRIGGER inventory_count_line_scope_snapshot_immutable BEFORE UPDATE ON inventory_count_lines
    FOR EACH ROW EXECUTE FUNCTION preserve_inventory_count_scope_snapshot();
