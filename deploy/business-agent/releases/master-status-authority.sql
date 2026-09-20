-- Rehearse on an isolated migrated database before production.
-- Copy only the four corresponding update grants, preserving every restriction.
-- Existing approval policies, Core permissions and object scopes are unchanged.
\set ON_ERROR_STOP on
BEGIN;
DO $$
DECLARE
 principal uuid;
 copied integer;
 trace uuid := gen_random_uuid();
 grants jsonb;
BEGIN
 IF (SELECT COALESCE(max(version),0) FROM _sqlx_migrations WHERE success) < 58 THEN
   RAISE EXCEPTION 'migration 58 is required';
 END IF;
 SELECT id INTO STRICT principal FROM business_iam.principals
   WHERE external_id='cd296cf8-922d-4299-b445-07b24e28d175' AND kind='human' AND status='active' FOR SHARE;
 PERFORM 1 FROM enterprise_users WHERE id='cd296cf8-922d-4299-b445-07b24e28d175' AND status='active' FOR SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'active enterprise user required'; END IF;
 PERFORM 1 FROM business_approval_policies WHERE action_code IN ('business_master_data:manage','business_product_master:manage') FOR SHARE;
 IF (SELECT count(*) FROM business_approval_policies WHERE action_code IN ('business_master_data:manage','business_product_master:manage') AND required_permission=action_code AND status='active') <> 2 THEN
   RAISE EXCEPTION 'both current master approval policies are required';
 END IF;
 PERFORM 1 FROM business_iam.principal_permissions g JOIN business_iam.permissions p ON p.id=g.permission_id
   WHERE g.principal_id=principal AND p.resource_type IN ('core_master_update_intent','product_master_update_intent') FOR SHARE OF g,p;
 IF (SELECT count(*) FROM business_iam.principal_permissions g JOIN business_iam.permissions p ON p.id=g.permission_id
     WHERE g.principal_id=principal AND p.resource_type IN ('core_master_update_intent','product_master_update_intent')
     AND p.action IN ('create','approve') AND p.status='active' AND g.valid_from<=now() AND (g.valid_until IS NULL OR g.valid_until>now())) <> 4 THEN
   RAISE EXCEPTION 'exactly four active corresponding update grants are required';
 END IF;
 IF EXISTS(SELECT 1 FROM business_iam.principal_permissions g JOIN business_iam.permissions p ON p.id=g.permission_id
     WHERE g.principal_id=principal AND p.resource_type IN ('core_master_status_intent','product_master_status_intent')) THEN
   RAISE EXCEPTION 'status grants already exist; inspect instead of overwriting';
 END IF;
 INSERT INTO business_iam.principal_permissions(principal_id,permission_id,data_scope,obligations,valid_from,valid_until,reason)
 SELECT principal,target.id,g.data_scope,g.obligations,g.valid_from,g.valid_until,'deployment:master-status'
 FROM business_iam.permissions target
 JOIN business_iam.permissions source ON source.capability=replace(target.capability,'_status_intent:','_update_intent:')
 JOIN business_iam.principal_permissions g ON g.permission_id=source.id AND g.principal_id=principal
 WHERE target.resource_type IN ('core_master_status_intent','product_master_status_intent')
   AND target.action IN ('create','approve') AND target.status='active' AND source.status='active'
   AND g.valid_from<=now() AND (g.valid_until IS NULL OR g.valid_until>now())
   AND (target.action<>'approve' OR target.obligations ? 'fresh_signed_chat_command');
 GET DIAGNOSTICS copied=ROW_COUNT;
 IF copied<>4 THEN RAISE EXCEPTION 'expected four status grants, got %',copied; END IF;
 SELECT jsonb_agg(jsonb_build_object('capability',p.capability,'dataScope',g.data_scope,'obligations',g.obligations,'validFrom',g.valid_from,'validUntil',g.valid_until) ORDER BY p.capability)
 INTO grants FROM business_iam.principal_permissions g JOIN business_iam.permissions p ON p.id=g.permission_id
 WHERE g.principal_id=principal AND p.resource_type IN ('core_master_status_intent','product_master_status_intent');
 INSERT INTO security_audit_events(id,event_type,result,trace_id,metadata)
 VALUES(gen_random_uuid(),'BUSINESS_IAM_ADMIN_MUTATION','success',trace,
 jsonb_build_object('actor','deployment:master-status','databasePrincipal',current_user,'operation','copy_master_status_grants','copiedGrants',copied,'grants',grants));
 RAISE NOTICE 'status grants copied=%, auditTrace=%',copied,trace;
END $$;
COMMIT;
