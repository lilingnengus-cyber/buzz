-- Legacy snapshots did not record their time basis. Keep it unknown rather than guessing.
ALTER TABLE operating_report_snapshots
 ADD COLUMN utc_offset_minutes smallint CHECK (utc_offset_minutes BETWEEN -720 AND 840),
 DROP CONSTRAINT operating_report_snapshots_cadence_period_start_currency_sc_key;
CREATE UNIQUE INDEX operating_snapshots_legacy_identity
 ON operating_report_snapshots(cadence,period_start,currency,scope_hash)
 WHERE utc_offset_minutes IS NULL;
CREATE UNIQUE INDEX operating_snapshots_zoned_identity
 ON operating_report_snapshots(cadence,period_start,currency,scope_hash,utc_offset_minutes)
 WHERE utc_offset_minutes IS NOT NULL;
