use super::*;
use business_core::b2::{CreateReturn, ReturnService};

// Gate INSERT after the source remainder check. This makes two shared-lock
// readers observe the same remainder deterministically; an exclusive source
// lock instead holds the second reader before that check.
pub(super) async fn check(store: &PgStore, f: &Fixture, sales: bool, input: &CreateReturn) {
    let actor = f.actor;
    for (delete,insert,scope) in [
        ("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2", "INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)",f.brand),
        ("DELETE FROM business_unit_scopes WHERE enterprise_user_id=$1 AND business_unit_id=$2", "INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)",f.business_unit),
    ] {
        sqlx::query(delete).bind(actor).bind(scope).execute(store.pool()).await.unwrap();
        let service=ReturnService::new(store.clone(),"SR".into(),"PR".into());
        let result=if sales {
            service.create_sales_return(actor,Uuid::new_v4(),"scope-denied-sales",input).await
        } else {
            service.create_purchase_return(actor,Uuid::new_v4(),"scope-denied-purchase",input).await
        };
        assert!(matches!(result,Err(DomainError::NotFoundOrForbidden)),"{result:?}");
        sqlx::query(insert).bind(actor).bind(scope).execute(store.pool()).await.unwrap();
    }

    sqlx::query("CREATE OR REPLACE FUNCTION test_return_insert_gate() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(918273645); RETURN NEW; END $$")
        .execute(store.pool()).await.unwrap();
    sqlx::query(if sales { "CREATE TRIGGER test_return_insert_gate BEFORE INSERT ON sales_returns FOR EACH ROW EXECUTE FUNCTION test_return_insert_gate()" } else { "CREATE TRIGGER test_return_insert_gate BEFORE INSERT ON purchase_returns FOR EACH ROW EXECUTE FUNCTION test_return_insert_gate()" })
        .execute(store.pool()).await.unwrap();
    let mut blocker = store.pool().begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(918273645)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let mut tasks = Vec::new();
    for index in 0..2 {
        let service = ReturnService::new(store.clone(), "SR".into(), "PR".into());
        let input = input.clone();
        tasks.push(tokio::spawn(async move {
            let key = format!("concurrent-return-{sales}-{index}");
            if sales {
                service
                    .create_sales_return(actor, Uuid::new_v4(), &key, &input)
                    .await
            } else {
                service
                    .create_purchase_return(actor, Uuid::new_v4(), &key, &input)
                    .await
            }
        }));
    }
    tokio::time::timeout(std::time::Duration::from_secs(10),async {
        loop {
            let waiting:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity a WHERE a.datname=current_database() AND ( $1=ANY(pg_blocking_pids(a.pid)) OR EXISTS(SELECT 1 FROM pg_stat_activity b WHERE b.datname=current_database() AND b.pid=ANY(pg_blocking_pids(a.pid)) AND $1=ANY(pg_blocking_pids(b.pid))))")
                .bind(blocker_pid).fetch_one(store.pool()).await.unwrap();
            if waiting>=2 {break;}
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("both create transactions must reach verified lock waits");
    blocker.commit().await.unwrap();
    let mut successful = Vec::new();
    let mut rejected = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(result) => successful.push(result),
            Err(DomainError::Invalid(message)) if message.contains("remainder") => rejected += 1,
            other => panic!("unexpected return result {other:?}"),
        }
    }
    sqlx::query(if sales {
        "DROP TRIGGER test_return_insert_gate ON sales_returns"
    } else {
        "DROP TRIGGER test_return_insert_gate ON purchase_returns"
    })
    .execute(store.pool())
    .await
    .unwrap();
    assert_eq!(
        successful.len(),
        1,
        "concurrent drafts cannot reserve the same last returnable unit twice"
    );
    assert_eq!(rejected, 1);
    let service = ReturnService::new(store.clone(), "SR".into(), "PR".into());
    let command = VersionCommand {
        expected_version: 1,
        reason_code: Some("test cleanup".into()),
    };
    let result = if sales {
        service
            .cancel_sales_return(
                actor,
                Uuid::new_v4(),
                successful[0].id,
                "cancel-race-sales",
                &command,
            )
            .await
    } else {
        service
            .cancel_purchase_return(
                actor,
                Uuid::new_v4(),
                successful[0].id,
                "cancel-race-purchase",
                &command,
            )
            .await
    };
    result.unwrap();
}
