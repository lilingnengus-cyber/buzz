use super::*;

pub(super) async fn place(
    sales: &SalesService,
    pool: &sqlx::PgPool,
    fixture: &Fixture,
    order_id: Uuid,
) -> business_core::b2::model::CommandResult {
    let hold_input = VersionCommand {
        expected_version: 2,
        reason_code: Some("CREDIT_REVIEW".into()),
    };
    let hold_preview = sales
        .hold_preview(fixture.actor, order_id, &hold_input, true)
        .await
        .unwrap();
    assert_eq!(hold_preview["canExecute"], true);
    let mut altered = hold_preview.clone();
    altered["reasonCode"] = serde_json::json!("OTHER_REASON");
    assert!(matches!(
        sales
            .set_hold_guarded(
                (fixture.actor, Uuid::new_v4()),
                order_id,
                "hold-tampered-preview",
                &hold_input,
                true,
                &altered
            )
            .await,
        Err(DomainError::StalePreview)
    ));
    let mut blocker = pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM sales_orders WHERE id=$1 FOR UPDATE")
        .bind(order_id)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let worker = sales.clone();
    let actor = fixture.actor;
    let guarded = hold_preview.clone();
    let pending = tokio::spawn(async move {
        worker
            .set_hold_guarded(
                (actor, Uuid::new_v4()),
                order_id,
                "hold-revoked-during-wait",
                &VersionCommand {
                    expected_version: 2,
                    reason_code: Some("CREDIT_REVIEW".into()),
                },
                true,
                &guarded,
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(pid)
            .fetch_one(pool)
            .await
            .unwrap();
            if blocked {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    sqlx::query("DELETE FROM business_role_permissions WHERE permission_key='sales_order:place_hold' AND role_id IN (SELECT role_id FROM business_user_roles WHERE enterprise_user_id=$1)").bind(actor).execute(pool).await.unwrap();
    blocker.commit().await.unwrap();
    assert!(matches!(
        pending.await.unwrap(),
        Err(DomainError::NotFoundOrForbidden)
    ));
    let state: (String, i64) =
        sqlx::query_as("SELECT hold_status,version FROM sales_orders WHERE id=$1")
            .bind(order_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(state, ("none".into(), 2));
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,'sales_order:place_hold' FROM business_user_roles WHERE enterprise_user_id=$1").bind(actor).execute(pool).await.unwrap();
    let hold = sales
        .set_hold_guarded(
            (fixture.actor, Uuid::new_v4()),
            order_id,
            "order-hold-0001",
            &hold_input,
            true,
            &hold_preview,
        )
        .await
        .unwrap();
    let replay = sales
        .set_hold_guarded(
            (fixture.actor, Uuid::new_v4()),
            order_id,
            "order-hold-0001",
            &hold_input,
            true,
            &hold_preview,
        )
        .await
        .unwrap();
    assert!(replay.idempotent_replay);
    assert_eq!(replay.version, hold.version);
    assert!(matches!(
        sales
            .set_hold_guarded(
                (fixture.actor, Uuid::new_v4()),
                order_id,
                "hold-stale-preview",
                &hold_input,
                true,
                &hold_preview
            )
            .await,
        Err(DomainError::StalePreview)
    ));
    assert!(matches!(
        sales
            .set_hold_guarded(
                (fixture.actor, Uuid::new_v4()),
                order_id,
                "order-hold-0001",
                &hold_input,
                true,
                &altered
            )
            .await,
        Err(DomainError::IdempotencyConflict)
    ));
    sqlx::query("DELETE FROM business_role_permissions WHERE permission_key='sales_order:place_hold' AND role_id IN (SELECT role_id FROM business_user_roles WHERE enterprise_user_id=$1)").bind(fixture.actor).execute(pool).await.unwrap();
    assert!(matches!(
        sales
            .set_hold_guarded(
                (fixture.actor, Uuid::new_v4()),
                order_id,
                "order-hold-0001",
                &hold_input,
                true,
                &hold_preview
            )
            .await,
        Err(DomainError::NotFoundOrForbidden)
    ));
    sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,'sales_order:place_hold' FROM business_user_roles WHERE enterprise_user_id=$1").bind(fixture.actor).execute(pool).await.unwrap();
    hold
}
