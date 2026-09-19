use super::*;
use axum::extract::Path;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn input(family: &str, id: Uuid) -> Value {
    match family {
        "creation" => {
            json!({"legalEntityId":id,"warehouseId":id,"countDate":"2026-09-20","currency":"CNY","skuIds":[id]})
        }
        "submission" => {
            json!({"inventoryCountId":id,"command":{"expectedVersion":1,"lines":[{"countLineId":id,"actualOnHandQuantity":"2","surplusUnitCost":"7"}]}})
        }
        "posting" => json!({"inventoryCountId":id,"command":{"expectedVersion":1}}),
        _ => json!({"inventoryCountId":id,"command":{"expectedVersion":1,"reasonCode":"重新盘点"}}),
    }
}
fn snapshot(family: &str, id: Uuid, command: &Value) -> Value {
    if family == "creation" {
        json!({"command":command,"businessUnitId":id,"lines":[{"skuId":id,"brandId":id}],"effect":"freeze_selected_inventory_until_count_posted_or_cancelled"})
    } else {
        json!({"source":{"id":id,"legalEntityId":id,"warehouseId":id,"businessUnitId":id,"snapshotBusinessUnitId":id,"version":1},"operation":command["operation"],"lines":[{"id":id,"brandId":id,"snapshotBrandId":id}]})
    }
}
fn envelope(kind: &str, id: Uuid, snapshot: &Value, trace: Uuid) -> Value {
    let hash: String = Sha256::digest(serde_json::to_vec(snapshot).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    json!({"item":{"id":id,"version":1,"snapshot":snapshot},"document":snapshot,"traceId":trace,"previewHash":hash,"approvalCommand":format!("确认 {} {id} v1 {hash}",kind.replace('_',"-")),"rejectionCommand":format!("拒绝 {} {id} v1 {hash}",kind.replace('_',"-"))})
}
fn context(scope: &str) -> RequestContext {
    RequestContext {
        enterprise_user_id: Uuid::new_v4(),
        identity_binding_id: Uuid::new_v4(),
        delegation_id: Uuid::new_v4(),
        agent_id: "count-agent".into(),
        agent_turn_id: "count-turn".into(),
        trace_id: Uuid::new_v4(),
        used_calls: 1,
        required_scope: scope.into(),
        source_buzz_event_id: "a".repeat(64),
        source_channel_id: "count-channel".into(),
    }
}
fn grant(context: &RequestContext, id: Uuid) -> EffectiveGrant {
    EffectiveGrant {
        capability: business_iam::Capability::parse(&context.required_scope).unwrap(),
        data_scope: DataScope::Restricted(BTreeMap::from([
            ("legal_entity".into(), [id.to_string()].into()),
            ("warehouse".into(), [id.to_string()].into()),
            ("business_unit".into(), [id.to_string()].into()),
            ("brand".into(), [id.to_string()].into()),
        ])),
        obligations: Default::default(),
    }
}
async fn serve(router: Router) -> (CoreClient, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (
        CoreClient {
            client: reqwest::Client::new(),
            base_url: Url::parse(&format!("http://{address}/")).unwrap(),
            credential: "count-test-credential".into(),
        },
        task,
    )
}
#[test]
fn inputs_bind_exact_operation_and_reject_model_execution_fields() {
    let id = Uuid::new_v4();
    for name in ["creation", "submission", "posting", "cancellation"] {
        let tool = format!("prepare_inventory_count_{name}");
        let value = input(name, id);
        assert!(valid_write_input(&tool, &value));
        assert_eq!(
            required_capability(&tool),
            Some(format!("inventory_count_{name}_intent:create").as_str())
        );
        assert!(WRITE_TOOLS.contains(&tool.as_str()));
        let mut extra = value.clone();
        extra["execute"] = true.into();
        assert!(!valid_write_input(&tool, &extra));
        if name != "creation" {
            let mut extra = value.clone();
            extra["command"]["snapshot"] = json!({});
            assert!(!valid_write_input(&tool, &extra));
            let mut zero = value.clone();
            zero["command"]["expectedVersion"] = 0.into();
            assert!(!valid_write_input(&tool, &zero));
        }
        let approved = format!("approve_inventory_count_{name}");
        let approval = json!({"documentId":id,"expectedVersion":1,"previewHash":"a".repeat(64),"decision":"approve"});
        assert!(valid_write_input(&approved, &approval));
        let mut changed = approval;
        changed["sourceBuzzEventId"] = "b".repeat(64).into();
        assert!(!valid_write_input(&approved, &changed));
    }
    let mut duplicate = input("submission", id);
    duplicate["command"]["lines"] = json!([
        duplicate["command"]["lines"][0],
        duplicate["command"]["lines"][0]
    ]);
    assert!(!valid("prepare_inventory_count_submission", &duplicate));
    let mut numeric = input("submission", id);
    numeric["command"]["lines"][0]["actualOnHandQuantity"] = 2.into();
    assert!(!valid("prepare_inventory_count_submission", &numeric));
}

#[tokio::test]
async fn preparations_bind_full_preview_scope_and_link_only_existing_counts() {
    for family_name in ["creation", "submission", "posting", "cancellation"] {
        for allowed in [true, false] {
            let id = Uuid::new_v4();
            let intent = Uuid::new_v4();
            let tool = format!("prepare_inventory_count_{family_name}");
            let (kind, category) = family(&tool).unwrap();
            let ctx = context(required_capability(&tool).unwrap());
            let input = input(family_name, id);
            let command = canonical(&tool, &input).unwrap();
            let snapshot = snapshot(family_name, id, &command);
            let trace = ctx.trace_id;
            let actor = ctx.enterprise_user_id;
            let prepared = envelope(kind, intent, &snapshot, trace);
            let expected_document = snapshot.clone();
            let expected_hash = prepared["previewHash"].clone();
            let posts = Arc::new(AtomicUsize::new(0));
            let observed = posts.clone();
            let key = format!("agent:{}:{tool}", ctx.delegation_id);
            let server = Router::new().route(
                "/v1/{phase}/{kind}",
                post(
                    move |Path((phase, actual_kind)): Path<(String, String)>,
                          headers: HeaderMap,
                          Json(body): Json<Value>| {
                        let snapshot = snapshot.clone();
                        let prepared = prepared.clone();
                        let command = command.clone();
                        let observed = observed.clone();
                        let key = key.clone();
                        async move {
                            assert_eq!(actual_kind, kind);
                            assert_eq!(headers["x-enterprise-user-id"], actor.to_string());
                            assert_eq!(headers["idempotency-key"], key);
                            assert_eq!(body, command);
                            if phase == format!("agent-{category}-previews") {
                                Json(json!({"document":snapshot,"traceId":trace}))
                            } else {
                                assert_eq!(phase, format!("agent-{category}-intents"));
                                observed.fetch_add(1, Ordering::SeqCst);
                                Json(prepared)
                            }
                        }
                    },
                ),
            );
            let (core, task) = serve(server).await;
            let response = forward(
                &core,
                &tool,
                input,
                &ctx,
                &grant(&ctx, if allowed { id } else { Uuid::new_v4() }),
            )
            .await;
            assert_eq!(
                response.status(),
                if allowed {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                }
            );
            assert_eq!(posts.load(Ordering::SeqCst), usize::from(allowed));
            if allowed {
                let body: Value = serde_json::from_slice(
                    &axum::body::to_bytes(response.into_body(), 65536)
                        .await
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(body["item"]["id"], json!(intent));
                assert_eq!(body["documentType"], kind);
                assert_eq!(body["item"]["status"], "draft");
                assert!(body["item"].get("snapshot").is_none());
                assert_eq!(body["document"], expected_document);
                assert_eq!(body["previewHash"], expected_hash);
                if family_name == "creation" {
                    assert_eq!(body["resourceRefs"], json!([]));
                } else {
                    assert_eq!(
                        body["resourceRefs"][0]["bizUri"],
                        format!("biz://inventory-count/{id}")
                    );
                }
            }
            task.abort();
        }
    }
}

#[tokio::test]
async fn count_approvals_recheck_frozen_scope_and_inject_only_verified_source() {
    for family_name in ["creation", "submission", "posting", "cancellation"] {
        for allowed in [true, false] {
            let id = Uuid::new_v4();
            let intent = Uuid::new_v4();
            let tool = format!("approve_inventory_count_{family_name}");
            let (kind, _) = family(&tool).unwrap();
            let ctx = context(required_capability(&tool).unwrap());
            let trace = ctx.trace_id;
            let input = canonical(
                &format!("prepare_inventory_count_{family_name}"),
                &input(family_name, id),
            )
            .unwrap();
            let mut snapshot = snapshot(family_name, id, &input);
            if !allowed {
                snapshot["lines"][0][if family_name == "creation" {
                    "brandId"
                } else {
                    "snapshotBrandId"
                }] = json!(Uuid::new_v4());
            }
            let prepared = envelope(kind, intent, &snapshot, trace);
            let hash = prepared["previewHash"].clone();
            let posts = Arc::new(AtomicUsize::new(0));
            let observed = posts.clone();
            let expected_hash = hash.clone();
            let category = if family_name == "creation" {
                "inventory-count-creations"
            } else {
                "inventory-count-operations"
            };
            let server=Router::new().route(&format!("/v1/agent-approval-previews/{category}/{kind}/{intent}"),get(move||{let prepared=prepared.clone();async move{Json(prepared)}}))
                .route(&format!("/v1/agent-approvals/{category}/{kind}/{intent}"),post(move|Json(body):Json<Value>|{let observed=observed.clone();let expected_hash=expected_hash.clone();async move{
                    observed.fetch_add(1,Ordering::SeqCst);assert_eq!(body,json!({"expectedVersion":1,"previewHash":expected_hash,"decision":"approve","sourceBuzzEventId":"a".repeat(64),"sourceChannelId":"count-channel"}));
                    let mut result=json!({"documentId":intent,"documentType":kind,"traceId":trace,"executed":true,"status":"executed"});
                    result[if family_name=="creation"{"createdDocument"}else{"updatedDocument"}]=json!({"id":id,"traceId":trace,"version":if family_name=="creation"{1}else{2},"status":match family_name {"creation"=>"counting","submission"=>"counted","posting"=>"posted",_=>"cancelled"}});Json(result)
                }}));
            let (core, task) = serve(server).await;
            let response=forward(&core,&tool,json!({"documentId":intent,"expectedVersion":1,"previewHash":hash,"decision":"approve"}),&ctx,&grant(&ctx,id)).await;
            assert_eq!(
                response.status(),
                if allowed {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                }
            );
            assert_eq!(posts.load(Ordering::SeqCst), usize::from(allowed));
            if allowed {
                let body: Value = serde_json::from_slice(
                    &axum::body::to_bytes(response.into_body(), 65536)
                        .await
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(body["resourceRefs"][0]["id"], json!(id));
            }
            task.abort();
        }
    }
}

#[test]
fn signed_preview_rejects_trace_hash_command_and_snapshot_substitution() {
    let id = Uuid::new_v4();
    let trace = Uuid::new_v4();
    let kind = "inventory_count_creation_intent";
    let snapshot = snapshot(
        "creation",
        id,
        &canonical("prepare_inventory_count_creation", &input("creation", id)).unwrap(),
    );
    let original = envelope(kind, Uuid::new_v4(), &snapshot, trace);
    assert!(bound_preview(&original, kind, trace));
    for field in [
        "traceId",
        "previewHash",
        "approvalCommand",
        "rejectionCommand",
    ] {
        let mut changed = original.clone();
        changed[field] = "substituted".into();
        assert!(!bound_preview(&changed, kind, trace));
    }
    let mut changed = original;
    changed["item"]["snapshot"]["command"]["warehouseId"] = json!(Uuid::new_v4());
    assert!(!bound_preview(&changed, kind, trace));
}

#[tokio::test]
async fn stale_approval_never_reaches_execution_route() {
    let id = Uuid::new_v4();
    let tool = "approve_inventory_count_creation";
    let ctx = context(required_capability(tool).unwrap());
    let (kind, _) = family(tool).unwrap();
    let snapshot = snapshot(
        "creation",
        id,
        &canonical("prepare_inventory_count_creation", &input("creation", id)).unwrap(),
    );
    let preview = envelope(kind, id, &snapshot, ctx.trace_id);
    let posts = Arc::new(AtomicUsize::new(0));
    let observed = posts.clone();
    let router = Router::new()
        .route(
            &format!("/v1/agent-approval-previews/inventory-count-creations/{kind}/{id}"),
            get(move || {
                let preview = preview.clone();
                async move { Json(preview) }
            }),
        )
        .fallback(move || {
            let observed = observed.clone();
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                StatusCode::INTERNAL_SERVER_ERROR
            }
        });
    let (core, task) = serve(router).await;
    let response=forward(&core,tool,json!({"documentId":id,"expectedVersion":1,"previewHash":"0".repeat(64),"decision":"approve"}),&ctx,&grant(&ctx,id)).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(posts.load(Ordering::SeqCst), 0);
    task.abort();
}

#[path = "inventory_count_writes_postgres.rs"]
mod postgres;

#[tokio::test]
async fn preview_pages_bind_hash_and_check_scope_outside_the_page() {
    for family_name in ["creation", "submission", "posting", "cancellation"] {
        for allowed in [true, false] {
            let id = Uuid::new_v4();
            let intent = Uuid::new_v4();
            let kind = format!("inventory_count_{family_name}_intent");
            let ctx = context("inventory:read");
            let command = canonical(
                &format!("prepare_inventory_count_{family_name}"),
                &input(family_name, id),
            )
            .unwrap();
            let mut snapshot = snapshot(family_name, id, &command);
            let mut second = snapshot["lines"][0].clone();
            second["id"] = json!(Uuid::new_v4());
            if !allowed {
                second["brandId"] = json!(Uuid::new_v4());
            }
            snapshot["lines"].as_array_mut().unwrap().push(second);
            let envelope = envelope(&kind, intent, &snapshot, ctx.trace_id);
            let hash = envelope["previewHash"].clone();
            let server = Router::new().route(
                "/{*path}",
                get(move || {
                    let envelope = envelope.clone();
                    async move { Json(envelope) }
                }),
            );
            let (core, task) = serve(server).await;
            let scope = iam_authorization_scope(&grant(&ctx, id), "inventory:read").unwrap();
            let input = json!({"documentId":intent,"documentType":kind,"previewHash":hash,"offset":0,"limit":1});
            let result = crate::inventory_count_previews::read(&core, &input, &scope, &ctx).await;
            assert_eq!(
                result.status(),
                if allowed {
                    StatusCode::OK
                } else {
                    StatusCode::NOT_FOUND
                }
            );
            if allowed {
                let bytes = axum::body::to_bytes(result.into_body(), 65536)
                    .await
                    .unwrap();
                let decoded: BusinessToolResult<Value> = serde_json::from_slice(&bytes).unwrap();
                assert!(decoded.pagination.unwrap().has_more);
                assert_eq!(
                    decoded.items[0]["document"]["lines"]
                        .as_array()
                        .unwrap()
                        .len(),
                    1
                );
                assert_eq!(decoded.items[0]["previewHash"], hash);
                assert_eq!(
                    decoded.items[0]["previewHashScope"],
                    "complete_server_snapshot"
                );
                let mut stale = input.clone();
                stale["previewHash"] = json!("0".repeat(64));
                assert_eq!(
                    crate::inventory_count_previews::read(&core, &stale, &scope, &ctx)
                        .await
                        .status(),
                    StatusCode::CONFLICT
                );
                let mut swapped = input.clone();
                swapped["documentId"] = json!(Uuid::new_v4());
                assert_eq!(
                    crate::inventory_count_previews::read(&core, &swapped, &scope, &ctx)
                        .await
                        .status(),
                    StatusCode::SERVICE_UNAVAILABLE
                );
            }
            task.abort();
        }
    }
}
