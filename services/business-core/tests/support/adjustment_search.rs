use super::*;
pub async fn verify(pool: &PgPool, app: &Router, f: &Fixture, order: Uuid) {
    let mut visible = Vec::new();
    for n in 0..3 {
        let id = fixture::draft(pool, f, order, &format!("search-visible-{n}")).await;
        sqlx::query("UPDATE operational_adjustment_batches SET adjustment_number=$2,created_at='2026-09-20 00:00:00+00'::timestamptz+($3::int*interval '1 minute') WHERE id=$1").bind(id).bind(format!("SEARCH-VISIBLE-{n}")).bind(n).execute(pool).await.unwrap();
        visible.push(id);
    }
    let hidden_brand = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO business_brands(id,code,name) VALUES($1,'SEARCH_HIDDEN','Hidden Brand')",
    )
    .bind(hidden_brand)
    .execute(pool)
    .await
    .unwrap();
    let mut hidden = None;
    for n in 0..65 {
        let id = Uuid::new_v4();
        hidden = Some(id);
        sqlx::query("INSERT INTO operational_adjustment_batches SELECT (jsonb_populate_record(NULL::operational_adjustment_batches,to_jsonb(b)||jsonb_build_object('id',$1::uuid,'adjustment_number',$2::text,'created_at','2026-09-20T01:00:00Z'))).* FROM operational_adjustment_batches b WHERE id=$3").bind(id).bind(format!("SEARCH-HIDDEN-{n}")).bind(visible[0]).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO operational_adjustment_lines SELECT (jsonb_populate_record(NULL::operational_adjustment_lines,to_jsonb(l)||jsonb_build_object('id',$1::uuid,'batch_id',$2::uuid,'brand_id',$3::uuid))).* FROM operational_adjustment_lines l WHERE batch_id=$4").bind(Uuid::new_v4()).bind(id).bind(hidden_brand).bind(visible[0]).execute(pool).await.unwrap();
    }
    let before:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM operational_adjustment_previews),(SELECT count(*) FROM business_core_audit_events),(SELECT count(*) FROM profit_facts)").fetch_one(pool).await.unwrap();
    let path =
        "/v1/profit-adjustments?number=SEARCH-&limit=2&status=draft&managementPeriod=2026-08";
    let (status, page) = call(app, f.actor, "GET", path, Value::Null, "").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["items"].as_array().unwrap().len(), 2);
    assert_eq!(page["items"][0]["id"], json!(visible[2]));
    assert_eq!(page["items"][1]["id"], json!(visible[1]));
    assert_eq!(page["pagination"]["hasMore"], true);
    assert_eq!(page["pagination"]["nextAfterId"], json!(visible[1]));
    assert!(page["pagination"].get("total").is_none());
    let (status, page) = call(
        app,
        f.actor,
        "GET",
        &format!("{path}&afterId={}", visible[1]),
        Value::Null,
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["items"][0]["id"], json!(visible[0]));
    assert_eq!(page["pagination"]["hasMore"], false);
    assert!(page["pagination"]["nextAfterId"].is_null());
    for id in [hidden.unwrap(), Uuid::new_v4()] {
        assert_eq!(
            call(
                app,
                f.actor,
                "GET",
                &format!("{path}&afterId={id}"),
                Value::Null,
                ""
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    for suffix in [
        "limit=0",
        "limit=101",
        "managementPeriod=2026-13",
        "status=unknown",
        "unexpected=1",
    ] {
        assert_eq!(
            call(
                app,
                f.actor,
                "GET",
                &format!("/v1/profit-adjustments?{suffix}"),
                Value::Null,
                ""
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    let (status, empty) = call(
        app,
        f.actor,
        "GET",
        "/v1/profit-adjustments?number=SEARCH-%25",
        Value::Null,
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty["items"], json!([]));
    let denied = format!("/v1/profit-adjustments?legalEntityId={}", Uuid::new_v4());
    assert_eq!(
        call(app, f.actor, "GET", &denied, Value::Null, "").await.0,
        StatusCode::NOT_FOUND
    );
    sqlx::query(
        "DELETE FROM business_customer_scopes WHERE enterprise_user_id=$1 AND customer_id=$2",
    )
    .bind(f.actor)
    .bind(f.customer)
    .execute(pool)
    .await
    .unwrap();
    let (status, empty) = call(app, f.actor, "GET", path, Value::Null, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty["items"], json!([]));
    assert_eq!(
        call(
            app,
            f.actor,
            "GET",
            &format!("{path}&afterId={}", visible[1]),
            Value::Null,
            ""
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO business_customer_scopes(enterprise_user_id,customer_id,granted_by) VALUES($1,$2,$1)").bind(f.actor).bind(f.customer).execute(pool).await.unwrap();
    let after:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM operational_adjustment_previews),(SELECT count(*) FROM business_core_audit_events),(SELECT count(*) FROM profit_facts)").fetch_one(pool).await.unwrap();
    assert_eq!(before, after);
}
