-- Reviewed one-time deployment operation. Run against the rehearsal DB first.
-- Copies current bounded inventory authority; never overwrites an existing grant/policy.
\set ON_ERROR_STOP on
BEGIN;
DO $$
DECLARE
  principal uuid;
  copied integer;
  source_policy jsonb;
  grant_snapshot jsonb;
  trace uuid := gen_random_uuid();
BEGIN
  IF (SELECT COALESCE(max(version),0) FROM _sqlx_migrations WHERE success) < 53 THEN
    RAISE EXCEPTION 'inventory count migrations through 53 are required';
  END IF;
  SELECT id INTO STRICT principal FROM business_iam.principals
    WHERE external_id='cd296cf8-922d-4299-b445-07b24e28d175' AND kind='human' AND status='active'
    FOR SHARE;
  PERFORM 1 FROM business_iam.principal_permissions g
    JOIN business_iam.permissions p ON p.id=g.permission_id
    WHERE g.principal_id=principal AND p.capability IN ('inventory_opening:create','inventory_opening:approve')
    FOR SHARE OF g,p;
  IF (SELECT count(*) FROM business_iam.principal_permissions g JOIN business_iam.permissions p ON p.id=g.permission_id
      WHERE g.principal_id=principal AND p.capability IN ('inventory_opening:create','inventory_opening:approve')
        AND p.status='active' AND g.valid_from<=now() AND (g.valid_until IS NULL OR g.valid_until>now())
        AND g.data_scope->>'mode'='restricted'
        AND g.data_scope->'dimensions'->'legal_entity'='["ea9d9cef-5408-4f86-a34c-afe4604f1754"]'::jsonb) <> 2 THEN
    RAISE EXCEPTION 'expected two active inventory grants restricted to the reviewed legal entity';
  END IF;
  IF EXISTS(SELECT 1 FROM business_iam.principal_permissions g JOIN business_iam.permissions p ON p.id=g.permission_id
      WHERE g.principal_id=principal AND p.resource_type IN ('inventory_count_creation_intent','inventory_count_submission_intent','inventory_count_posting_intent','inventory_count_cancellation_intent')) THEN
    RAISE EXCEPTION 'count grants already exist; inspect instead of overwriting';
  END IF;
  IF EXISTS(SELECT 1 FROM business_approval_policies WHERE action_code='inventory_opening:create') THEN
    RAISE EXCEPTION 'creation policy already exists; inspect instead of overwriting';
  END IF;
  SELECT to_jsonb(policy) INTO STRICT source_policy FROM business_approval_policies policy
    WHERE action_code='inventory_opening:post' AND required_permission='inventory_opening:post' AND status='active'
    FOR SHARE;
  INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval,require_distinct_business_unit,step_up_amount_minor,status)
    SELECT 'inventory_opening:create','inventory_opening:create',eligible_role_keys,min_approvers,allow_self_approval,require_distinct_business_unit,step_up_amount_minor,status
    FROM business_approval_policies WHERE action_code='inventory_opening:post';
  INSERT INTO business_iam.principal_permissions(principal_id,permission_id,data_scope,obligations,valid_from,valid_until,reason)
    SELECT principal,target.id,g.data_scope,g.obligations,g.valid_from,g.valid_until,'deployment:inventory-counts-e7c54b8c1'
    FROM business_iam.permissions target
    JOIN business_iam.permissions parent ON parent.capability='inventory_opening:'||target.action
    JOIN business_iam.principal_permissions g ON g.permission_id=parent.id AND g.principal_id=principal
    WHERE target.resource_type IN ('inventory_count_creation_intent','inventory_count_submission_intent','inventory_count_posting_intent','inventory_count_cancellation_intent')
      AND target.action IN ('create','approve') AND target.status='active'
      AND (target.action='create' OR target.obligations ? 'fresh_signed_chat_command');
  GET DIAGNOSTICS copied = ROW_COUNT;
  IF copied <> 8 THEN RAISE EXCEPTION 'expected exactly eight fixed count grants, got %',copied; END IF;
  SELECT jsonb_agg(jsonb_build_object('capability',p.capability,'dataScope',g.data_scope,'obligations',g.obligations,'validFrom',g.valid_from,'validUntil',g.valid_until) ORDER BY p.capability)
    INTO grant_snapshot FROM business_iam.principal_permissions g JOIN business_iam.permissions p ON p.id=g.permission_id
    WHERE g.principal_id=principal AND g.reason='deployment:inventory-counts-e7c54b8c1';
  INSERT INTO security_audit_events(id,event_type,result,trace_id,metadata)
    VALUES(gen_random_uuid(),'BUSINESS_IAM_ADMIN_MUTATION','success',trace,
      jsonb_build_object('actor','deployment:inventory-counts-e7c54b8c1','databasePrincipal',current_user,
        'operation','inventory_count_policy_and_grants','externalId','cd296cf8-922d-4299-b445-07b24e28d175',
        'copiedGrants',copied,'grants',grant_snapshot,'sourcePolicy',source_policy,'targetPolicy','inventory_opening:create',
        'parentCapabilities',jsonb_build_array('inventory_opening:create','inventory_opening:approve')));
  RAISE NOTICE 'count authority prepared: grants=%, auditTrace=%',copied,trace;
END $$;
COMMIT;
