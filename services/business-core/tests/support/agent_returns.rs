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
    serde_json::from_value(result).unwrap()
}
