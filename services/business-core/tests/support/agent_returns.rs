use super::*;

pub(super) async fn create(
    app: &Router,
    store: &PgStore,
    f: &Fixture,
    sales: bool,
    input: &business_core::b2::CreateReturn,
) -> business_core::b2::model::CommandResult {
    let kind = if sales {
        "sales_return"
    } else {
        "purchase_return"
    };
    let source = format!("/v1/agent-return-sources/{kind}/{}", input.source_id);
    let path = format!("/v1/agent-drafts/returns/{kind}");
    let (status, detail) = call(app, f.actor, "GET", &source, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["item"]["version"], 2);
    assert_eq!(
        detail["item"]["lines"][0]["sourceLineId"],
        input.lines[0].source_line_id.to_string()
    );
    assert_eq!(
        detail["item"]["lines"][0]["returnableQuantity"]
            .as_str()
            .unwrap()
            .parse::<Decimal>()
            .unwrap(),
        Decimal::ONE
    );
    assert!(detail["item"]["lines"][0]["sourceAmount"].is_string());
    let mut draft = serde_json::to_value(input).unwrap();
    assert_eq!(
        call(app, f.actor, "POST", &path, draft.clone()).await.0,
        StatusCode::BAD_REQUEST
    );
    draft["expectedSourceVersion"] = json!(1);
    assert_eq!(
        call(app, f.actor, "POST", &path, draft.clone()).await.0,
        StatusCode::CONFLICT
    );
    draft["expectedSourceVersion"] = detail["item"]["version"].clone();
    let mut unknown = draft.clone();
    unknown["execute"] = json!(true);
    assert_eq!(
        call(app, f.actor, "POST", &path, unknown).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    for (delete,insert,scope) in [
        ("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2","INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)",f.brand),
        ("DELETE FROM business_unit_scopes WHERE enterprise_user_id=$1 AND business_unit_id=$2","INSERT INTO business_unit_scopes(enterprise_user_id,business_unit_id,granted_by) VALUES($1,$2,$1)",f.business_unit),
        ("DELETE FROM business_warehouse_scopes WHERE enterprise_user_id=$1 AND warehouse_id=$2","INSERT INTO business_warehouse_scopes(enterprise_user_id,warehouse_id,granted_by) VALUES($1,$2,$1)",f.warehouse),
    ] {
        sqlx::query(delete).bind(f.actor).bind(scope).execute(store.pool()).await.unwrap();
        assert_eq!(call(app,f.actor,"GET",&source,Value::Null).await.0,StatusCode::NOT_FOUND);
        assert_eq!(call(app,f.actor,"POST",&path,draft.clone()).await.0,StatusCode::NOT_FOUND);
        sqlx::query(insert).bind(f.actor).bind(scope).execute(store.pool()).await.unwrap();
    }
    let key = format!("agent-return-create-{kind}");
    let (status, result) = call_key(app, f.actor, "POST", &path, draft.clone(), &key).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["status"], "draft");
    assert_eq!(result["version"], 1);
    let (status, replay) = call_key(app, f.actor, "POST", &path, draft.clone(), &key).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["idempotentReplay"], true);
    assert_eq!(replay["id"], result["id"]);
    draft["reasonCode"] = json!("changed reason");
    assert_eq!(
        call_key(app, f.actor, "POST", &path, draft, &key).await.0,
        StatusCode::CONFLICT
    );
    let (_, after) = call(app, f.actor, "GET", &source, Value::Null).await;
    assert_eq!(
        after["item"]["lines"][0]["returnableQuantity"]
            .as_str()
            .unwrap()
            .parse::<Decimal>()
            .unwrap(),
        Decimal::ZERO
    );
    let lookup = format!(
        "/v1/agent-return-documents/{kind}?documentId={}&limit=1",
        result["id"].as_str().unwrap()
    );
    let (status, found) = call(app, f.actor, "GET", &lookup, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(found["items"].as_array().unwrap().len(), 1);
    assert_eq!(found["items"][0]["version"], 1);
    let detail_path = format!(
        "/api/v1/{}-returns/{}",
        if sales { "sales" } else { "purchase" },
        result["id"].as_str().unwrap()
    );
    let browser_token = browser_session(store, f.actor).await;
    assert_eq!(
        call(app, f.actor, "GET", &detail_path, Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, detail) = browser_read(app, &detail_path, &browser_token).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["id"], result["id"]);
    assert_eq!(detail["lines"], found["items"][0]["lines"]);
    assert_eq!(detail["sourceId"], input.source_id.to_string());

    assert_eq!(
        found["items"][0]["lines"][0]["sourceLineId"],
        input.lines[0].source_line_id.to_string()
    );
    assert!(found["items"][0]["lines"][0]["returnLineId"].is_string());
    assert!(found["items"][0]["lines"][0]["quantity"].is_string());
    let (_, by_number) = call(
        app,
        f.actor,
        "GET",
        &format!(
            "{lookup}&query={}&status=draft",
            result["number"].as_str().unwrap().to_lowercase()
        ),
        Value::Null,
    )
    .await;
    assert_eq!(by_number["items"], found["items"]);
    let (_, past) = call(
        app,
        f.actor,
        "GET",
        &format!("{lookup}&offset=1"),
        Value::Null,
    )
    .await;
    assert_eq!(past["items"], json!([]));
    let (_, wrong_party) = call(
        app,
        f.actor,
        "GET",
        &format!("{lookup}&partyId={}", Uuid::new_v4()),
        Value::Null,
    )
    .await;
    assert_eq!(wrong_party["items"], json!([]));
    assert_eq!(
        call(
            app,
            f.actor,
            "GET",
            &format!("{lookup}&extra=1"),
            Value::Null
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    sqlx::query("DELETE FROM business_brand_scopes WHERE enterprise_user_id=$1 AND brand_id=$2")
        .bind(f.actor)
        .bind(f.brand)
        .execute(store.pool())
        .await
        .unwrap();
    let (_, hidden) = call(app, f.actor, "GET", &lookup, Value::Null).await;
    assert_eq!(hidden["items"], json!([]));
    assert_eq!(
        browser_read(app, &detail_path, &browser_token).await.0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO business_brand_scopes(enterprise_user_id,brand_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.brand).execute(store.pool()).await.unwrap();
    serde_json::from_value(result).unwrap()
}

async fn browser_session(store: &PgStore, actor: Uuid) -> String {
    let workbench = Uuid::new_v4();
    let embed = Uuid::new_v4();
    let trace = Uuid::new_v4();
    let token = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO workbench_sessions(id,enterprise_user_id,status,expires_at,trace_id) VALUES($1,$2,'active',now()+interval '1 hour',$3)")
        .bind(workbench).bind(actor).bind(trace).execute(store.pool()).await.unwrap();
    sqlx::query("INSERT INTO embed_sessions(id,code_hash,enterprise_user_id,identity_binding_id,workbench_session_id,audience,deployment_id,target_path,target_resource_type,target_resource_id,status,expires_at,trace_id) VALUES($1,$2,$3,NULL,$4,'business-dock','integration','/','business_home','home','consumed',now()+interval '1 hour',$5)")
        .bind(embed).bind(business_auth_gateway::security::hash(&token)).bind(actor).bind(workbench).bind(trace).execute(store.pool()).await.unwrap();
    sqlx::query("INSERT INTO business_sessions(id,session_token_hash,csrf_token_hash,enterprise_user_id,identity_binding_id,workbench_session_id,embed_session_id,status,expires_at,trace_id) VALUES($1,$2,$2,$3,NULL,$4,$5,'active',now()+interval '1 hour',$6)")
        .bind(Uuid::new_v4()).bind(business_auth_gateway::security::hash(&token)).bind(actor).bind(workbench).bind(embed).bind(trace).execute(store.pool()).await.unwrap();
    token
}
async fn browser_read(app: &Router, path: &str, token: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::get(path)
                .header("cookie", format!("__Host-bizfin_business={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}
