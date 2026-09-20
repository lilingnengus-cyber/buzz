use super::*;
use crate::store::ApprovalPolicy;
use sqlx::{Postgres, Transaction};

async fn voter(
    state: &AppState,
    tx: &mut Transaction<'_, Postgres>,
    command: &Command,
    snapshot: &Value,
    policy: &ApprovalPolicy,
    people: (Uuid, Uuid),
) -> Result<Option<chrono::DateTime<chrono::Utc>>, StoreError> {
    let (actor, creator) = people;
    // Identity status is not covered by every scope-revision trigger. Hold the
    // actual identities and roles, not only the authorization revision row.
    let users: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM enterprise_users WHERE id=ANY($1) AND status='active' ORDER BY id FOR SHARE")
        .bind(vec![actor,creator]).fetch_all(&mut **tx).await?;
    if !users.contains(&actor) || !users.contains(&creator) {
        return Err(StoreError::NotFoundOrForbidden);
    }
    sqlx::query("SELECT r.id FROM business_roles r JOIN business_user_roles ur ON ur.role_id=r.id WHERE ur.enterprise_user_id=$1 ORDER BY r.id FOR SHARE OF r")
        .bind(actor).fetch_all(&mut **tx).await?;
    let authority = PgStore::snapshot_on(tx, actor).await?;
    if policy.step_up_amount_minor.is_some()
        || (!policy.allow_self_approval && actor == creator)
        || !authority
            .permission_keys
            .contains(&policy.required_permission)
        || !authority
            .roles
            .iter()
            .any(|role| policy.eligible_role_keys.contains(&role.role_key))
    {
        return Err(StoreError::NotFoundOrForbidden);
    }
    if policy.require_distinct_business_unit {
        let requester = PgStore::snapshot_on(tx, creator).await?;
        if requester.scopes.business_unit_ids.is_empty()
            || authority.scopes.business_unit_ids.is_empty()
            || !requester
                .scopes
                .business_unit_ids
                .is_disjoint(&authority.scopes.business_unit_ids)
        {
            return Err(StoreError::NotFoundOrForbidden);
        }
    }
    let policy_deadline = authority::permission(tx, actor, &policy.required_permission).await?;
    let write_deadline = authority::permission(tx, actor, command.action()).await?;
    let creator_deadline = authority::permission(tx, creator, command.action()).await?;
    // Recheck both parties on every vote, including votes below the threshold.
    if command.preview_on(&state.sales, tx, actor).await? != *snapshot
        || command.preview_on(&state.sales, tx, creator).await? != *snapshot
    {
        return Err(StoreError::Conflict);
    }
    Ok(policy_deadline
        .into_iter()
        .chain(write_deadline)
        .chain(creator_deadline)
        .min())
}

pub(super) async fn execute(
    state: &AppState,
    context: (Uuid, Uuid),
    kind: &str,
    id: Uuid,
    input: &ChatApprovalInput,
) -> Result<Value, StoreError> {
    validate_input(input)?;
    let (actor, trace) = context;
    let mut tx = state.store.pool().begin().await?;
    let (command, snapshot, creator) = load(&mut tx, kind, id).await?;
    if input.expected_version != 1 || input.preview_hash != hash_json(&snapshot) {
        return Err(StoreError::Conflict);
    }
    if command.preview_on(&state.sales, &mut tx, actor).await? != snapshot {
        return Err(StoreError::Conflict);
    }
    let policy: ApprovalPolicy = sqlx::query_as("SELECT required_permission,eligible_role_keys,min_approvers,allow_self_approval,require_distinct_business_unit,step_up_amount_minor FROM business_approval_policies WHERE action_code=$1 AND status='active' FOR SHARE")
        .bind(command.action()).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFoundOrForbidden)?;
    let mut deadlines = vec![
        voter(
            state,
            &mut tx,
            &command,
            &snapshot,
            &policy,
            (actor, creator),
        )
        .await?,
    ];
    let request: Uuid = sqlx::query_scalar("INSERT INTO business_document_approval_requests(id,document_type,document_id,action_code,expected_version,preview_hash,requester_user_id,minimum_approvers,trace_id) VALUES($1,$2,$3,$4,1,$5,$6,$7,$8) ON CONFLICT(document_type,document_id,expected_version,preview_hash) DO UPDATE SET preview_hash=business_document_approval_requests.preview_hash RETURNING id")
        .bind(Uuid::new_v4()).bind(kind).bind(id).bind(command.action()).bind(&input.preview_hash).bind(creator).bind(policy.min_approvers).bind(trace).fetch_one(&mut *tx).await?;
    let row = sqlx::query("SELECT status,minimum_approvers FROM business_document_approval_requests WHERE id=$1 FOR UPDATE").bind(request).fetch_one(&mut *tx).await?;
    if row.get::<String, _>("status") != "pending" {
        return Err(StoreError::Conflict);
    }
    // A later policy may strengthen the threshold, but never erase approvals
    // originally required by this request. Revalidate every earlier voter.
    let minimum = row
        .get::<i16, _>("minimum_approvers")
        .max(policy.min_approvers);
    let prior: Vec<Uuid> = sqlx::query_scalar("SELECT approver_user_id FROM business_document_approval_votes WHERE request_id=$1 AND decision='approve' ORDER BY approver_user_id")
        .bind(request).fetch_all(&mut *tx).await?;
    if prior.contains(&actor) {
        return Err(StoreError::Conflict);
    }
    if input.decision == ApprovalDecision::Approve {
        for approver in &prior {
            deadlines.push(
                voter(
                    state,
                    &mut tx,
                    &command,
                    &snapshot,
                    &policy,
                    (*approver, creator),
                )
                .await?,
            );
        }
    }
    let inserted = sqlx::query("INSERT INTO business_document_approval_votes(id,request_id,approver_user_id,decision,source_buzz_event_id,source_channel_id,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING")
        .bind(Uuid::new_v4()).bind(request).bind(actor).bind(if input.decision == ApprovalDecision::Approve {"approve"} else {"reject"}).bind(&input.source_buzz_event_id).bind(&input.source_channel_id).bind(trace).execute(&mut *tx).await?.rows_affected();
    if inserted != 1 {
        return Err(StoreError::Conflict);
    }
    let count = prior.len() as i64 + i64::from(input.decision == ApprovalDecision::Approve);
    let execute = input.decision == ApprovalDecision::Approve && count >= i64::from(minimum);
    let created = if execute {
        let result = command
            .save_on(&state.sales, &mut tx, context, request, &snapshot)
            .await?;
        Some(result)
    } else {
        None
    };
    let status = if execute {
        "executed"
    } else if input.decision == ApprovalDecision::Reject {
        "rejected"
    } else {
        "pending"
    };
    let changed = sqlx::query("UPDATE business_document_approval_requests r SET status=$2,minimum_approvers=$3,decided_at=CASE WHEN $2<>'pending' THEN now() ELSE decided_at END,executed_at=CASE WHEN $2='executed' THEN now() ELSE executed_at END,version=version+1 WHERE id=$1 AND status='pending' AND EXISTS(SELECT 1 FROM business_agent_order_hold_intents i WHERE i.id=r.document_id AND i.kind=r.document_type AND i.expires_at>clock_timestamp())")
        .bind(request).bind(status).bind(minimum).execute(&mut *tx).await?.rows_affected();
    if changed != 1 {
        return Err(StoreError::Conflict);
    }
    sqlx::query("INSERT INTO business_core_audit_events(trace_id,actor_user_id,operation,target_type,target_id,details) VALUES($1,$2,'chat_document_approval_vote',$3,$4,$5)")
        .bind(trace).bind(actor).bind(kind).bind(id.to_string()).bind(json!({"requestId":request,"decision":input.decision,"approvalCount":count,"minimumApprovers":minimum,"sourceBuzzEventId":input.source_buzz_event_id})).execute(&mut *tx).await?;
    // Do this after all writes and waits: wall-clock expiry is not protected by row locks.
    let deadline = deadlines.into_iter().flatten().min();
    let current: bool = sqlx::query_scalar("SELECT ($1::timestamptz IS NULL OR $1>clock_timestamp()) AND EXISTS(SELECT 1 FROM business_agent_order_hold_intents WHERE id=$2 AND expires_at>clock_timestamp())")
        .bind(deadline).bind(id).fetch_one(&mut *tx).await?;
    if !current {
        return Err(StoreError::NotFoundOrForbidden);
    }
    tx.commit().await?;
    Ok(
        json!({"documentId":id,"documentType":kind,"requestId":request,"status":status,"executed":execute,"createdDocument":created,"approvalCount":count,"minimumApprovers":minimum,"traceId":trace}),
    )
}
