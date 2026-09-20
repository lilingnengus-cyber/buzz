-- Preserve the actual report scope independently of authorization revision.
-- Do not guess historical scopes from current grants or rewrite immutable rows.
ALTER TABLE operating_report_snapshots ADD COLUMN snapshot_scope jsonb
 CHECK(snapshot_scope IS NULL OR jsonb_typeof(snapshot_scope)='object');
CREATE INDEX operating_snapshots_owner_trend_idx ON operating_report_snapshots
 (generated_by_user_id,cadence,currency,period_start DESC);
