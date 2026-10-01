-- The first core-master numbering seed declared legal-entity pools without
-- including the scope segment required to make rendered codes unambiguous.
UPDATE business_numbering_rules
SET segments = jsonb_insert(
    jsonb_insert(segments, '{1}', '{"type":"scope"}'::jsonb, false),
    '{2}',
    '{"type":"fixed","value":"-"}'::jsonb,
    false
)
WHERE record_type IN ('customer', 'supplier', 'warehouse')
  AND scope_dimension = 'legal_entity'
  AND NOT segments @> '[{"type":"scope"}]'::jsonb;
