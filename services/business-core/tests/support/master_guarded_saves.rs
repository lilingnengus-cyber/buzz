use super::{preview, DomainError, PgPool, Uuid, Value};
use business_core::{
    master_data::CoreMasterDataService, product_master::ProductMasterService, PgStore,
};
use serde_json::json;

async fn execute(
    pool: &PgPool,
    actor: Uuid,
    product: bool,
    key: &str,
    command: Value,
    snapshot: &Value,
) -> Result<Value, DomainError> {
    if product {
        let result = ProductMasterService::new(PgStore::new(pool.clone()))
            .save_guarded(
                actor,
                Uuid::new_v4(),
                key,
                &serde_json::from_value(command).unwrap(),
                snapshot,
            )
            .await?;
        Ok(serde_json::to_value(result).unwrap())
    } else {
        let result = CoreMasterDataService::new(PgStore::new(pool.clone()))
            .save_guarded(
                actor,
                Uuid::new_v4(),
                key,
                &serde_json::from_value(command).unwrap(),
                snapshot,
            )
            .await?;
        Ok(serde_json::to_value(result).unwrap())
    }
}

pub async fn check(
    pool: &PgPool,
    actor: Uuid,
    entries: &[(bool, Uuid, Value)],
    conversion_unit: Uuid,
) {
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM business_core_audit_events")
        .fetch_one(pool)
        .await
        .unwrap();
    // Every type executes both create and update; replay uses the original preview
    // even though execution has necessarily changed its record or impact counts.
    for (product, id, input) in entries {
        let mut fresh = input.clone();
        if input["resourceType"] == "uom_conversion" {
            fresh["unitOfMeasureId"] = json!(conversion_unit);
        } else {
            fresh["code"] = json!(format!("{}_GUARDED", input["code"].as_str().unwrap()));
        }
        if input["resourceType"] == "sku" {
            fresh["barcode"] = json!("PV_GUARDED_BARCODE");
        }
        let create = json!({"operation":"create","command":fresh});
        let snapshot = preview(pool, actor, *product, create.clone())
            .await
            .unwrap();
        let key = Uuid::new_v4().to_string();
        let saved = execute(pool, actor, *product, &key, create.clone(), &snapshot)
            .await
            .unwrap();
        assert_eq!(saved["version"], 1);
        assert_eq!(saved["idempotentReplay"], false);
        let replay = execute(pool, actor, *product, &key, create.clone(), &snapshot)
            .await
            .unwrap();
        assert_eq!(replay["id"], saved["id"]);
        assert_eq!(replay["traceId"], saved["traceId"]);
        assert_eq!(replay["idempotentReplay"], true);
        let mut changed = snapshot.clone();
        changed["canExecute"] = json!(false);
        assert!(matches!(
            execute(pool, actor, *product, &key, create, &changed).await,
            Err(DomainError::IdempotencyConflict)
        ));
        let status = json!({"operation":"change_status","resourceType":input["resourceType"],"documentId":id,"command":{"status":"disabled","expectedVersion":1}});
        assert!(matches!(
            execute(
                pool,
                actor,
                *product,
                &Uuid::new_v4().to_string(),
                status,
                &snapshot
            )
            .await,
            Err(DomainError::Invalid(_))
        ));
        let version: i64 = if *product {
            sqlx::query_scalar("SELECT version FROM product_master_data_maintenance WHERE resource_type=$1 AND id=$2").bind(input["resourceType"].as_str().unwrap()).bind(id).fetch_one(pool).await.unwrap()
        } else {
            sqlx::query_scalar(
                "SELECT version FROM core_master_data_maintenance WHERE resource_type=$1 AND id=$2",
            )
            .bind(input["resourceType"].as_str().unwrap())
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
        };
        let mut update = input.clone();
        update["expectedVersion"] = json!(version);
        if input["resourceType"] != "uom_conversion" {
            update["name"] = json!("Guarded update");
        }
        let command = json!({"operation":"update","documentId":id,"command":update});
        let snapshot = preview(pool, actor, *product, command.clone())
            .await
            .unwrap();
        let key = Uuid::new_v4().to_string();
        let mut changed = snapshot.clone();
        changed["effectiveFields"]["name"] = json!("Unconfirmed change");
        assert!(matches!(
            execute(pool, actor, *product, &key, command.clone(), &changed).await,
            Err(DomainError::StalePreview)
        ));
        // Rejected execution leaves no idempotency reservation behind.
        let saved = execute(pool, actor, *product, &key, command.clone(), &snapshot)
            .await
            .unwrap();
        assert_eq!(saved["version"], version + 1);
        assert_eq!(
            execute(pool, actor, *product, &key, command.clone(), &snapshot)
                .await
                .unwrap()["idempotentReplay"],
            true
        );
        assert!(matches!(
            execute(
                pool,
                actor,
                *product,
                &Uuid::new_v4().to_string(),
                command,
                &snapshot
            )
            .await,
            Err(DomainError::VersionConflict)
        ));
    }
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM business_core_audit_events")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(after - before, 22);
    for kind in ["customer", "sku"] {
        let (product, id, input) = entries
            .iter()
            .find(|(_, _, v)| v["resourceType"] == kind)
            .unwrap();
        let mut update = input.clone();
        update["expectedVersion"] = json!(2);
        let command = json!({"operation":"update","documentId":id,"command":update});
        let snapshot = preview(pool, actor, *product, command.clone())
            .await
            .unwrap();
        // Wait for a real parent lock, then observe a committed parent change.
        let mut lock = pool.begin().await.unwrap();
        let parent = if *product {
            input["productId"].as_str().unwrap()
        } else {
            input["legalEntityId"].as_str().unwrap()
        };
        let parent = Uuid::parse_str(parent).unwrap();
        if *product {
            sqlx::query("UPDATE business_products SET name=name WHERE id=$1")
                .bind(parent)
                .execute(&mut *lock)
                .await
                .unwrap();
        } else {
            sqlx::query("UPDATE business_legal_entities SET name=name WHERE id=$1")
                .bind(parent)
                .execute(&mut *lock)
                .await
                .unwrap();
        }
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        let p = pool.clone();
        let c = command.clone();
        let s = snapshot.clone();
        let is_product = *product;
        let task = tokio::spawn(async move {
            execute(&p, actor, is_product, &Uuid::new_v4().to_string(), c, &s).await
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))").bind(blocker).fetch_one(pool).await.unwrap();
                if waiting { break; }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        lock.commit().await.unwrap();
        assert!(matches!(
            task.await.unwrap(),
            Err(DomainError::StalePreview)
        ));
        let snapshot = preview(pool, actor, *product, command.clone())
            .await
            .unwrap();
        if *product {
            let brand = entries
                .iter()
                .find(|(_, _, v)| v["resourceType"] == "brand")
                .unwrap()
                .1;
            sqlx::query(
                "DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2",
            )
            .bind(actor)
            .bind(brand)
            .execute(pool)
            .await
            .unwrap();
        } else {
            sqlx::query("DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2").bind(actor).bind(id).execute(pool).await.unwrap();
        }
        assert!(matches!(
            execute(
                pool,
                actor,
                *product,
                &Uuid::new_v4().to_string(),
                command,
                &snapshot
            )
            .await,
            Err(DomainError::NotFoundOrForbidden)
        ));
    }
    let final_count: i64 = sqlx::query_scalar("SELECT count(*) FROM business_core_audit_events")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(final_count, after);
}
