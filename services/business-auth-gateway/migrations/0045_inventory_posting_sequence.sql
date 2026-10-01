-- Transaction timestamps cannot establish stock posting order: a transaction may
-- begin before another transaction, wait for its balance lock, then post later.
-- Assign an immutable sequence at INSERT while the stock writer holds that lock.
-- Do not invent ordering for historical rows; automatic reversal must fail closed
-- when its original movements lack this evidence.
CREATE SEQUENCE business_inventory_posting_sequence;
ALTER TABLE inventory_movements ADD COLUMN posting_sequence BIGINT;
ALTER TABLE inventory_movements ALTER COLUMN posting_sequence
 SET DEFAULT nextval('business_inventory_posting_sequence');
ALTER SEQUENCE business_inventory_posting_sequence OWNED BY inventory_movements.posting_sequence;
CREATE UNIQUE INDEX inventory_movements_posting_sequence_unique
 ON inventory_movements(posting_sequence) WHERE posting_sequence IS NOT NULL;
CREATE INDEX inventory_movements_scope_posting_sequence
 ON inventory_movements(legal_entity_id,warehouse_id,sku_id,posting_sequence);
-- Enforce sequence evidence for every new row without rewriting historical rows.
ALTER TABLE inventory_movements ADD CONSTRAINT inventory_movements_new_posting_sequence
 CHECK (posting_sequence IS NOT NULL AND posting_sequence > 0) NOT VALID;
