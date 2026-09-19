use super::*;
use business_core::PgStore;
use sqlx::PgPool;
#[path = "integration.rs"]
mod integration;
fn context(actor: Uuid, tool: &str) -> RequestContext {
    RequestContext {
        enterprise_user_id: actor,
        identity_binding_id: Uuid::new_v4(),
        delegation_id: Uuid::new_v4(),
        agent_id: "master-adapter-test".into(),
        agent_turn_id: "master-turn".into(),
        trace_id: Uuid::new_v4(),
        used_calls: 1,
        required_scope: required_capability(tool).unwrap().into(),
        source_buzz_event_id: Uuid::new_v4().simple().to_string().repeat(2),
        source_channel_id: "isolated-master-adapter".into(),
    }
}
fn grant(c: &RequestContext, dimensions: &[(&str, Uuid)]) -> EffectiveGrant {
    EffectiveGrant {
        capability: business_iam::Capability::parse(&c.required_scope).unwrap(),
        data_scope: if dimensions.is_empty() {
            DataScope::Unrestricted
        } else {
            DataScope::Restricted(
                dimensions
                    .iter()
                    .map(|(k, id)| (k.to_string(), [id.to_string()].into()))
                    .collect(),
            )
        },
        obligations: Default::default(),
    }
}
async fn value(response: Response) -> Value {
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 131072)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status, StatusCode::OK, "{value}");
    value
}
#[test]
fn fixed_patch_inputs_preserve_omission_and_reject_unrelated_fields() {
    let id = Uuid::new_v4();
    let input = json!({"documentId":id,"resourceType":"customer","expectedVersion":2,"changes":{"name":"Renamed"}});
    assert!(valid("prepare_core_master_update", &input));
    for changes in [
        json!({}),
        json!({"legalEntityId":id}),
        json!({"code":"CHANGED"}),
        json!({"paymentTermsDays":null}),
        json!({"barcode":"wrong type field"}),
    ] {
        let mut bad = input.clone();
        bad["changes"] = changes;
        assert!(!valid("prepare_core_master_update", &bad));
    }
    assert!(!valid("prepare_product_master_update", &input));
    let patch: Patch = serde_json::from_value(input).unwrap();
    let record = json!({"id":id,"resourceType":"customer","version":2,"code":"C","name":"Old","legalEntityId":id,"businessUnitId":id,"creditCurrency":"CNY","creditLimitMinor":34567,"paymentTermsDays":45});
    let command = patch.merge(&record).unwrap();
    assert_eq!(command["command"]["creditLimitMinor"], 34567);
    assert_eq!(command["command"]["paymentTermsDays"], 45);
    let patch:Patch=serde_json::from_value(json!({"documentId":id,"resourceType":"sku","expectedVersion":2,"changes":{"barcode":null}})).unwrap();
    assert!(patch.valid("product"));
    let command=patch.merge(&json!({"resourceType":"sku","id":id,"version":2,"productId":id,"code":"S","name":"SKU","barcode":"OLD"})).unwrap();
    assert!(command["command"]["barcode"].is_null());
    for family in ["core", "product"] {
        for operation in ["creation", "update"] {
            let tool = format!("approve_{family}_master_{operation}");
            let mut input = json!({"documentId":id,"expectedVersion":1,"previewHash":"a".repeat(64),"decision":"approve"});
            assert!(valid(&tool, &input));
            input["sourceBuzzEventId"] = json!("b".repeat(64));
            assert!(!valid(&tool, &input));
            assert!(WRITE_TOOLS.contains(&tool.as_str()));
        }
    }
}
#[test]
fn missing_dimensions_never_satisfy_restricted_creation_or_global_authority() {
    let id = Uuid::new_v4().to_string();
    for scope in [
        AuthorizationScope {
            legal_entity_ids: [id.clone()].into(),
            ..Default::default()
        },
        AuthorizationScope {
            brand_ids: [id.clone()].into(),
            ..Default::default()
        },
        AuthorizationScope {
            customer_ids: [id.clone()].into(),
            ..Default::default()
        },
    ] {
        for kind in [
            "legal_entity",
            "brand",
            "unit_of_measure",
            "product_category",
        ] {
            assert!(!scope::record(
                &json!({"resourceType":kind,"id":null,"legalEntityId":null,"brandId":null}),
                &scope
            ));
        }
    }
    let scoped = AuthorizationScope {
        legal_entity_ids: [id.clone()].into(),
        business_unit_ids: [id.clone()].into(),
        ..Default::default()
    };
    assert!(scope::record(
        &json!({"resourceType":"customer","id":null,"legalEntityId":id,"businessUnitId":id}),
        &scoped
    ));
    let scoped = AuthorizationScope {
        customer_ids: [id.clone()].into(),
        ..scoped
    };
    assert!(!scope::record(
        &json!({"resourceType":"customer","id":null,"legalEntityId":id,"businessUnitId":id}),
        &scoped
    ));
    assert!(scope::record(
        &json!({"resourceType":"customer","id":id,"legalEntityId":id,"businessUnitId":id}),
        &scoped
    ));
}

#[test]
fn duplicate_scope_aliases_cannot_union_different_write_ranges() {
    let c = context(Uuid::new_v4(), "prepare_core_master_update");
    let bad = grant(
        &c,
        &[
            ("legal_entity", Uuid::new_v4()),
            ("legalEntityIds", Uuid::new_v4()),
        ],
    );
    assert!(authorization_scope(&bad, &c.required_scope).is_none());
    let valid = grant(&c, &[("legalEntityIds", Uuid::new_v4())]);
    assert!(authorization_scope(&valid, &c.required_scope).is_some());
}
