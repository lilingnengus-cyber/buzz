use super::*;

pub async fn check(
    pool: &PgPool,
    app: &Router,
    actor: Uuid,
    role: Uuid,
    entries: &[(bool, Uuid, Value)],
) {
    let approver = Uuid::new_v4();
    sqlx::query("INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,display_name) VALUES($1,'requester-access',$1::text,'Approver')").bind(approver).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO business_user_roles(enterprise_user_id,role_id,assigned_by) VALUES($1,$2,$3)",
    )
    .bind(approver)
    .bind(role)
    .bind(actor)
    .execute(pool)
    .await
    .unwrap();
    for (table, column) in [
        ("business_legal_entity_scopes", "legal_entity_id"),
        ("business_unit_scopes", "business_unit_id"),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!("INSERT INTO {table}(enterprise_user_id,{column},granted_by) SELECT $1,{column},$2 FROM {table} WHERE enterprise_user_id=$2"))).bind(approver).bind(actor).execute(pool).await.unwrap();
    }
    sqlx::query("UPDATE business_approval_policies SET min_approvers=1,allow_self_approval=false,require_distinct_business_unit=false,step_up_amount_minor=NULL WHERE action_code='business_master_data:manage'").execute(pool).await.unwrap();
    for (_, _, fields) in entries.iter().filter(|(product, _, _)| !product) {
        let mut fields = fields.clone();
        fields["code"] = json!(Uuid::new_v4().simple().to_string().to_uppercase());
        let kind = fields["resourceType"].as_str().unwrap().to_owned();
        let prepared = prepare(
            app,
            actor,
            "core_master_creation_intent",
            json!({"operation":"create","command":fields}),
        )
        .await;
        let (status, result) = call(
            app,
            approver,
            "POST",
            &approval_path("core_master_creation_intent", &prepared),
            vote(&prepared),
            "",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        let id = result["createdDocument"]["id"].as_str().unwrap();
        for reader in [actor, approver] {
            let (status, detail) = call(
                app,
                reader,
                "GET",
                &format!("/v1/agent-core-master-records/{kind}/{id}"),
                Value::Null,
                "",
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{detail}");
        }
        // The access change is attributed to the approving user and exact requester.
        let requester_grant:i64=sqlx::query_scalar("SELECT count(*) FROM business_core_audit_events WHERE operation='agent_master_requester_access' AND target_id=$1 AND actor_user_id=$2 AND details->>'requesterUserId'=$3")
            .bind(id).bind(approver).bind(actor.to_string()).fetch_one(pool).await.unwrap();
        assert_eq!(requester_grant, 1);
    }
    let mut fields = entries
        .iter()
        .find(|(_, _, v)| v["resourceType"] == "customer")
        .unwrap()
        .2
        .clone();
    fields["code"] = json!(Uuid::new_v4().simple().to_string().to_uppercase());
    let unit = Uuid::parse_str(fields["businessUnitId"].as_str().unwrap()).unwrap();
    let prepared = prepare(
        app,
        actor,
        "core_master_creation_intent",
        json!({"operation":"create","command":fields}),
    )
    .await;
    sqlx::query(
        "DELETE FROM business_unit_scopes WHERE enterprise_user_id=$1 AND business_unit_id=$2",
    )
    .bind(actor)
    .bind(unit)
    .execute(pool)
    .await
    .unwrap();
    let counts="SELECT (SELECT count(*) FROM business_customers),(SELECT count(*) FROM business_customer_scopes),(SELECT count(*) FROM business_document_approval_votes),(SELECT count(*) FROM business_core_audit_events)";
    let before: (i64, i64, i64, i64) = sqlx::query_as(sqlx::AssertSqlSafe(counts))
        .fetch_one(pool)
        .await
        .unwrap();
    let (status, _) = call(
        app,
        approver,
        "POST",
        &approval_path("core_master_creation_intent", &prepared),
        vote(&prepared),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let after: (i64, i64, i64, i64) = sqlx::query_as(sqlx::AssertSqlSafe(counts))
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(before, after);
    sqlx::query("INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)").bind(actor).bind(unit).execute(pool).await.unwrap();
    sqlx::query("UPDATE business_approval_policies SET allow_self_approval=true WHERE action_code='business_master_data:manage'").execute(pool).await.unwrap();
}
