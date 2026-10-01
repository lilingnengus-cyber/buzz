-- Sequence allocation is not commit order. A late committed profit fact may
-- change the report contents without increasing the maximum visible sequence.
-- Preserve immutable older reports while allowing a new content revision.
ALTER TABLE management_report_snapshots
 DROP CONSTRAINT management_report_snapshots_report_type_management_period_c_key,
 ADD CONSTRAINT management_report_snapshots_content_identity_key
 UNIQUE(report_type,management_period,currency,scope_hash,rule_version,source_watermark,source_hash);
