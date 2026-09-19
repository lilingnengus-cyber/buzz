use super::*;
use sha2::{Digest, Sha256};

pub(super) async fn check(
    core: &CoreClient,
    pool: &PgPool,
    actor: Uuid,
    cookie: &str,
    entries: &[(&str, Uuid)],
) {
    let workbench = Uuid::new_v4();
    let embed = Uuid::new_v4();
    let trace = Uuid::new_v4();
    let token = Uuid::new_v4().to_string();
    let hash = Sha256::digest(token.as_bytes()).to_vec();
    sqlx::query("INSERT INTO workbench_sessions(id,enterprise_user_id,status,expires_at,trace_id) VALUES($1,$2,'active',now()+interval '1 hour',$3)")
        .bind(workbench).bind(actor).bind(trace).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO embed_sessions(id,code_hash,enterprise_user_id,identity_binding_id,workbench_session_id,audience,deployment_id,target_path,target_resource_type,target_resource_id,status,expires_at,trace_id) VALUES($1,$2,$3,NULL,$4,'business-dock','integration','/embed/','business_home','home','consumed',now()+interval '1 hour',$5)")
        .bind(embed).bind(&hash).bind(actor).bind(workbench).bind(trace).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO business_sessions(id,session_token_hash,csrf_token_hash,enterprise_user_id,identity_binding_id,workbench_session_id,embed_session_id,status,expires_at,trace_id) VALUES($1,$2,$2,$3,NULL,$4,$5,'active',now()+interval '1 hour',$6)")
        .bind(Uuid::new_v4()).bind(&hash).bind(actor).bind(workbench).bind(embed).bind(trace).execute(pool).await.unwrap();
    for (kind, id) in entries {
        let family = input::family_of_resource(kind).unwrap();
        let url = core
            .base_url
            .join(&format!("api/v1/{family}-master-data/{kind}/{id}"))
            .unwrap();
        let absent = core.client.get(url.clone()).send().await.unwrap();
        assert_eq!(absent.status(), reqwest::StatusCode::UNAUTHORIZED);
        let response = core
            .client
            .get(url.clone())
            .header("cookie", format!("{cookie}={token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK, "{kind}");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["item"]["id"], json!(id));
        assert_eq!(body["item"]["resourceType"], *kind);
        // An expired workbench session invalidates the detail link immediately.
        sqlx::query(
            "UPDATE workbench_sessions SET expires_at=now()-interval '1 second' WHERE id=$1",
        )
        .bind(workbench)
        .execute(pool)
        .await
        .unwrap();
        let expired = core
            .client
            .get(url)
            .header("cookie", format!("{cookie}={token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(expired.status(), reqwest::StatusCode::UNAUTHORIZED);
        sqlx::query("UPDATE workbench_sessions SET expires_at=now()+interval '1 hour' WHERE id=$1")
            .bind(workbench)
            .execute(pool)
            .await
            .unwrap();
    }
}
