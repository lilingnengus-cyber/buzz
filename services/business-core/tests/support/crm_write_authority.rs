use business_core::{
    b2::DomainError,
    crm::{AddFollowup, CrmService, SaveOpportunity},
    PgStore,
};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn check(pool: &PgPool, actor: Uuid, customer: Uuid, id: Uuid) {
    for save in [false, true] {
        sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)")
            .bind(actor).bind(customer).execute(pool).await.unwrap();
        let crm = CrmService::new(PgStore::new(pool.clone()));
        let before = crm.detail(actor, id, 0).await.unwrap();
        let version = before["item"]["version"].as_i64().unwrap();
        let mut lock = pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM crm_opportunities WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        let input: SaveOpportunity = serde_json::from_value(serde_json::json!({
            "legalEntityId":before["item"]["legalEntityId"],
            "businessUnitId":before["item"]["businessUnitId"],
            "customerId":if save { None } else { Some(customer) },
            "title":"must roll back","companyName":"test","stage":"contacting",
            "currency":"CNY","expectedVersion":version
        }))
        .unwrap();
        let task = tokio::spawn(async move {
            if save {
                crm.save(actor, Uuid::new_v4(), Some(id), "crm-revoke-save", &input)
                    .await
            } else {
                crm.followup(
                    actor,
                    Uuid::new_v4(),
                    id,
                    "crm-revoke-followup",
                    &AddFollowup {
                        note: "must roll back".into(),
                        stage: "contacting".into(),
                        next_action: "must roll back".into(),
                        next_follow_up: None,
                        expected_version: version,
                    },
                )
                .await
            }
        });
        // Observe the actual blocked UPDATE, not a timing assumption.
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let waiting: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) AND (query LIKE 'UPDATE crm_opportunities%' OR query LIKE 'SELECT * FROM crm_opportunities%'))",
                ).bind(blocker).fetch_one(pool).await.unwrap();
                if waiting { break; }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        sqlx::query(
            "DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2",
        )
        .bind(actor)
        .bind(customer)
        .execute(pool)
        .await
        .unwrap();
        lock.commit().await.unwrap();
        assert!(matches!(
            task.await.unwrap(),
            Err(DomainError::NotFoundOrForbidden)
        ));
        sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)")
            .bind(actor).bind(customer).execute(pool).await.unwrap();
        let crm = CrmService::new(PgStore::new(pool.clone()));
        assert_eq!(crm.detail(actor, id, 0).await.unwrap(), before);
        let writes: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM business_core_audit_events WHERE target_type='crm_opportunity'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(writes, 3);
        sqlx::query(
            "DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2",
        )
        .bind(actor)
        .bind(customer)
        .execute(pool)
        .await
        .unwrap();
    }
}
