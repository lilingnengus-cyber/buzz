-- Reviewed one-time deployment operation. Run against the rehearsal DB first.
-- Core-family grants keep the reviewed legal entity. Product-family grants are global
-- only because products have no legal entity; Core still checks current brand scope.
-- This grants no new Core permission or object scope and does not change existing read grants.
-- Creating a new legal entity is outside this deployment scope and remains unconfigured.
-- Copies approval restrictions and grant conditions; never overwrites grants or policies.
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
  IF (SELECT COALESCE(max(version),0) FROM _sqlx_migrations WHERE success) < 57 THEN
    RAISE EXCEPTION 'master-data migrations through 57 are required';
  END IF;
  SELECT id INTO STRICT principal FROM business_iam.principals
    WHERE external_id='cd296cf8-922d-4299-b445-07b24e28d175' AND kind='human' AND status='active'
    FOR SHARE;
  PERFORM 1 FROM enterprise_users WHERE id='cd296cf8-922d-4299-b445-07b24e28d175' AND status='active' FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'active enterprise user required'; END IF;
  PERFORM 1 FROM business_iam.principal_permissions g
    JOIN business_iam.permissions p ON p.id=g.permission_id
    WHERE g.principal_id=principal AND p.capability IN ('sales_order:approve')
    FOR SHARE OF g,p;
  IF (SELECT count(*) FROM business_iam.principal_permissions g JOIN business_iam.permissions p ON p.id=g.permission_id
      WHERE g.principal_id=principal AND p.capability IN ('sales_order:approve')
        AND p.status='active' AND g.valid_from<=now() AND (g.valid_until IS NULL OR g.valid_until>now())
        AND g.data_scope->>'mode'='restricted'
        AND g.data_scope->'dimensions'=jsonb_build_object('legal_entity',jsonb_build_array('ea9d9cef-5408-4f86-a34c-afe4604f1754'))
        AND g.data_scope->'dimensions'->'legal_entity'='["ea9d9cef-5408-4f86-a34c-afe4604f1754"]'::jsonb) <> 1 THEN
    RAISE EXCEPTION 'expected one active sales approval grant restricted to the reviewed legal entity';
  END IF;
  -- Core master-data authority remains independently enforced on every operation.
  PERFORM 1 FROM business_authorization_revision WHERE singleton FOR SHARE;
  IF (SELECT count(DISTINCT p.permission_key) FROM business_user_roles u
      JOIN business_roles r ON r.id=u.role_id JOIN business_role_permissions p ON p.role_id=r.id
      WHERE u.enterprise_user_id='cd296cf8-922d-4299-b445-07b24e28d175' AND r.status='active'
        AND p.permission_key IN ('business_master_data:read','business_master_data:manage','business_product_master:read','business_product_master:manage')) <> 4
     OR NOT EXISTS(SELECT 1 FROM business_legal_entity_scopes WHERE enterprise_user_id='cd296cf8-922d-4299-b445-07b24e28d175' AND legal_entity_id='ea9d9cef-5408-4f86-a34c-afe4604f1754')
     OR NOT EXISTS(SELECT 1 FROM business_unit_scopes WHERE enterprise_user_id='cd296cf8-922d-4299-b445-07b24e28d175' AND business_unit_id='e6e6045b-1b70-4270-b9b7-8439461c9b0a')
     OR NOT EXISTS(SELECT 1 FROM business_customer_scopes WHERE enterprise_user_id='cd296cf8-922d-4299-b445-07b24e28d175' AND customer_id='622e0e24-2ab1-42cb-954d-2220c581accf') THEN
    RAISE EXCEPTION 'existing Core master-data authority and reviewed dimensions required';
  END IF;
  IF EXISTS(SELECT 1 FROM business_iam.principal_permissions g JOIN business_iam.permissions p ON p.id=g.permission_id
      WHERE g.principal_id=principal AND p.resource_type IN ('core_master_creation_intent','core_master_update_intent','product_master_creation_intent','product_master_update_intent','business_product_master')) THEN
    RAISE EXCEPTION 'master-data grants already exist; inspect instead of overwriting';
  END IF;
  IF EXISTS(SELECT 1 FROM business_approval_policies WHERE action_code IN ('business_master_data:manage','business_product_master:manage')) THEN
    RAISE EXCEPTION 'master-data policy already exists; inspect instead of overwriting';
  END IF;
  SELECT to_jsonb(policy) INTO STRICT source_policy FROM business_approval_policies policy
    WHERE action_code='sales_order:confirm' AND required_permission='sales_order:confirm' AND status='active'
    FOR SHARE;
  INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval,require_distinct_business_unit,step_up_amount_minor,status)
    SELECT target.action_code,target.action_code,eligible_role_keys,min_approvers,allow_self_approval,require_distinct_business_unit,step_up_amount_minor,status
    FROM business_approval_policies CROSS JOIN (VALUES ('business_master_data:manage'),('business_product_master:manage')) target(action_code)
    WHERE business_approval_policies.action_code='sales_order:confirm';
  INSERT INTO business_iam.principal_permissions(principal_id,permission_id,data_scope,obligations,valid_from,valid_until,reason)
    SELECT principal,target.id,
      CASE WHEN target.resource_type IN ('core_master_creation_intent','core_master_update_intent') THEN g.data_scope
           ELSE '{"mode":"unrestricted"}'::jsonb END,
      g.obligations,g.valid_from,g.valid_until,'deployment:master-e51a84b9c'
    FROM business_iam.permissions target
    JOIN business_iam.permissions parent ON parent.capability='sales_order:approve'
    JOIN business_iam.principal_permissions g ON g.permission_id=parent.id AND g.principal_id=principal
    WHERE (target.resource_type IN ('core_master_creation_intent','core_master_update_intent','product_master_creation_intent','product_master_update_intent') AND target.action IN ('create','approve') OR target.capability='business_product_master:read') AND target.status='active'
      AND (target.action<>'approve' OR target.obligations ? 'fresh_signed_chat_command');
  GET DIAGNOSTICS copied = ROW_COUNT;
  IF copied <> 9 THEN RAISE EXCEPTION 'expected exactly nine fixed master-data grants, got %',copied; END IF;
  SELECT jsonb_agg(jsonb_build_object('capability',p.capability,'dataScope',g.data_scope,'obligations',g.obligations,'validFrom',g.valid_from,'validUntil',g.valid_until) ORDER BY p.capability)
    INTO grant_snapshot FROM business_iam.principal_permissions g JOIN business_iam.permissions p ON p.id=g.permission_id
    WHERE g.principal_id=principal AND g.reason='deployment:master-e51a84b9c';
  INSERT INTO security_audit_events(id,event_type,result,trace_id,metadata)
    VALUES(gen_random_uuid(),'BUSINESS_IAM_ADMIN_MUTATION','success',trace,
      jsonb_build_object('actor','deployment:master-e51a84b9c','databasePrincipal',current_user,
        'operation','master_policy_and_grants','externalId','cd296cf8-922d-4299-b445-07b24e28d175',
        'copiedGrants',copied,'grants',grant_snapshot,'sourcePolicy',source_policy,'targetPolicies',jsonb_build_array('business_master_data:manage','business_product_master:manage'),
        'parentCapabilities',jsonb_build_array('sales_order:approve')));
  RAISE NOTICE 'master-data authority prepared: grants=%, auditTrace=%',copied,trace;
END $$;
COMMIT;
