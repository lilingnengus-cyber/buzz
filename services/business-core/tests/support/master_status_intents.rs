use super::*;

pub(super) async fn check(
    pool: &PgPool,
    app: &Router,
    actor: Uuid,
    entries: &[(bool, Uuid, Value)],
) {
    for (resource, table, kind) in [
        (
            "customer",
            "business_customers",
            "core_master_status_intent",
        ),
        ("sku", "business_skus", "product_master_status_intent"),
    ] {
        let id = entries
            .iter()
            .find(|(_, _, fields)| fields["resourceType"] == resource)
            .unwrap()
            .1;
        for status in ["disabled", "active"] {
            let v: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT version FROM {table} WHERE id=$1"
            )))
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap();
            let command = json!({"operation":"change_status","resourceType":resource,"documentId":id,"command":{"status":status,"expectedVersion":v}});
            let prepared = prepare(app, actor, kind, command.clone()).await;
            let mut wrong = vote(&prepared);
            wrong["command"] = json!({"status":"active"});
            assert_eq!(
                call(
                    app,
                    actor,
                    "POST",
                    &approval_path(kind, &prepared),
                    wrong,
                    ""
                )
                .await
                .0,
                StatusCode::UNPROCESSABLE_ENTITY
            );
            let path = approval_path(kind, &prepared);
            let (code, result) = call(app, actor, "POST", &path, vote(&prepared), "").await;
            assert_eq!(code, StatusCode::OK, "{result}");
            assert_eq!(result["executed"], true);
            assert_ne!(
                call(app, actor, "POST", &path, vote(&prepared), "").await.0,
                StatusCode::OK
            );
            let actual: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT status FROM {table} WHERE id=$1"
            )))
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap();
            assert_eq!(actual, status);
        }
    }
    // A brand with an active product cannot be disabled; failed execution must not retain a vote.
    let id = entries
        .iter()
        .find(|(_, _, fields)| fields["resourceType"] == "brand")
        .unwrap()
        .1;
    let v: i64 = sqlx::query_scalar("SELECT version FROM business_brands WHERE id=$1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    let prepared=prepare(app,actor,"product_master_status_intent",json!({"operation":"change_status","resourceType":"brand","documentId":id,"command":{"status":"disabled","expectedVersion":v}})).await;
    assert_eq!(prepared["document"]["canExecute"], false);
    let before: (i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM business_document_approval_votes),(SELECT count(*) FROM business_document_approval_requests)")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(
        call(
            app,
            actor,
            "POST",
            &approval_path("product_master_status_intent", &prepared),
            vote(&prepared),
            ""
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let after: (i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM business_document_approval_votes),(SELECT count(*) FROM business_document_approval_requests)")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(before, after);
    let status: String = sqlx::query_scalar("SELECT status FROM business_brands WHERE id=$1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(status, "active");
}
