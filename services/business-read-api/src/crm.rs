//! Fixed CRM adapter, preserving signed-turn actor identity and server-derived command keys.
use super::*;
use business_query_contracts::{crm::*, ValidateInput};

pub(super) const READS: [&str; 2] = ["search_crm_leads", "get_crm_lead"];
pub(super) const WRITES: [&str; 3] = [
    "create_crm_lead",
    "record_crm_lead_followup",
    "convert_crm_lead",
];

pub(super) fn valid_write(tool: &str, input: &Value) -> bool {
    match tool {
        "create_crm_lead" => serde_json::from_value::<CreateCrmLeadInput>(input.clone()).is_ok(),
        "record_crm_lead_followup" => {
            serde_json::from_value::<RecordCrmLeadFollowupInput>(input.clone())
                .is_ok_and(|i| i.expected_version > 0)
        }
        "convert_crm_lead" => serde_json::from_value::<ConvertCrmLeadInput>(input.clone())
            .is_ok_and(|i| i.confirmed && i.expected_version > 0),
        _ => false,
    }
}
fn endpoint(tool: &str, input: &Value) -> Option<String> {
    if matches!(tool, "search_crm_leads" | "create_crm_lead") {
        return Some("v1/agent-crm/leads".into());
    }
    let id = input.get("leadId")?.as_str()?.parse::<Uuid>().ok()?;
    let suffix = match tool {
        "get_crm_lead" => "",
        "record_crm_lead_followup" => "/followups",
        "convert_crm_lead" => "/convert",
        _ => return None,
    };
    Some(format!("v1/agent-crm/leads/{id}{suffix}"))
}
pub(super) async fn read(
    core: &CoreClient,
    tool: &str,
    input: &Value,
    context: &RequestContext,
) -> Response {
    let valid = if tool == "search_crm_leads" {
        serde_json::from_value::<SearchCrmLeadsInput>(input.clone()).is_ok_and(|mut i| {
            i.validate_and_normalize(chrono::Utc::now().date_naive())
                .is_ok()
        })
    } else {
        serde_json::from_value::<GetCrmLeadInput>(input.clone()).is_ok()
    };
    if !valid {
        return (StatusCode::BAD_REQUEST, "invalid_filter").into_response();
    }
    let Some(path) = endpoint(tool, input) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Ok(mut url) = core.base_url.join(&path) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if tool == "search_crm_leads" {
        let mut query = url.query_pairs_mut();
        for key in ["query", "status", "ownerUserId", "dueBy", "offset"] {
            if let Some(value) = input.get(key).filter(|v| !v.is_null()) {
                query.append_pair(
                    key,
                    &value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string()),
                );
            }
        }
    }
    let request = core
        .client
        .get(url)
        .header("x-business-service-credential", &core.credential)
        .header("x-service-audience", "business-core")
        .header(
            "x-enterprise-user-id",
            context.enterprise_user_id.to_string(),
        )
        .header("x-trace-id", context.trace_id.to_string());
    let Ok(response) = request.send().await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let status = response.status();
    if !status.is_success() {
        return status.into_response();
    }
    let Ok(value) = response.json::<Value>().await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let items: Vec<Value> = if tool == "get_crm_lead" {
        value.get("item").cloned().into_iter().collect()
    } else {
        value
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let mut result = BusinessToolResult::empty(BusinessToolStatus::Ok, context.trace_id);
    let mut sanitized = Vec::new();
    for item in items {
        let Some(id) = item
            .get("id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
        else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let title = item.get("title").and_then(Value::as_str).unwrap_or("线索");
        result.resource_refs.push(ResourceRef {
            r#type: "crm_lead".into(),
            id: Some(id.to_string()),
            title: title
                .chars()
                .filter(|c| !c.is_control())
                .take(160)
                .collect(),
            biz_uri: format!("biz://crm-lead/{id}"),
        });
        // Contact details and freeform history stay in the authorized Dock page.
        let mut reduced = item.clone();
        if let Some(map) = reduced.as_object_mut() {
            map.remove("contactDetails");
            map.remove("summary");
        }
        sanitized.push(reduced);
    }
    result.items = sanitized;
    let more = tool == "search_crm_leads"
        && value
            .get("hasMore")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    result.pagination = Some(Pagination {
        has_more: more,
        next_cursor: more.then(|| {
            format!(
                "offset:{}",
                input.get("offset").and_then(Value::as_i64).unwrap_or(0) + 50
            )
        }),
    });
    result.summary.insert(
        "nextOffset".into(),
        if more {
            json!(input.get("offset").and_then(Value::as_i64).unwrap_or(0) + 50)
        } else {
            Value::Null
        },
    );
    Json(result).into_response()
}
pub(super) async fn write(
    core: &CoreClient,
    tool: &str,
    mut input: Value,
    context: &RequestContext,
) -> Response {
    if context.source_buzz_event_id.len() != 64
        || !context
            .source_buzz_event_id
            .bytes()
            .all(|c| c.is_ascii_hexdigit())
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(path) = endpoint(tool, &input) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let expected_status = match tool {
        "create_crm_lead" => "new".to_owned(),
        "record_crm_lead_followup" => input
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        "convert_crm_lead" => "contacting".to_owned(),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let Some(map) = input.as_object_mut() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    map.remove("leadId");
    map.remove("confirmed");
    if tool == "convert_crm_lead" {
        let Some(opportunity) = map.get_mut("opportunity").and_then(Value::as_object_mut) else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        opportunity.insert("stage".into(), json!("contacting"));
    }
    let Ok(url) = core.base_url.join(&path) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Ok(response) = core
        .client
        .post(url)
        .header("x-business-service-credential", &core.credential)
        .header("x-service-audience", "business-core")
        .header(
            "x-enterprise-user-id",
            context.enterprise_user_id.to_string(),
        )
        .header("x-trace-id", context.trace_id.to_string())
        .header(
            "idempotency-key",
            format!("agent:{}:{tool}", context.source_buzz_event_id),
        )
        .json(&input)
        .send()
        .await
    else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let status = response.status();
    let Ok(mut value) = response.json::<Value>().await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if !status.is_success() {
        return (status, Json(value)).into_response();
    }
    let Some(id) = value
        .get("id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
    else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    value["commandTraceId"] = value["traceId"].clone();
    value["traceId"] = json!(context.trace_id);
    value["status"] = json!(expected_status);
    let (kind, resource_type) = if tool == "convert_crm_lead" {
        ("crm-opportunity", "crm_opportunity")
    } else {
        ("crm-lead", "crm_lead")
    };
    Json(json!({"schemaVersion":1,"status":"ok","item":value,"resourceRefs":[{"type":resource_type,"id":id,"title":if tool=="convert_crm_lead" {"打开已转换的商机"} else {"打开线索"},"bizUri":format!("biz://{kind}/{id}")}],"traceId":context.trace_id})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    #[tokio::test]
    async fn crm_adapter_forwards_only_fixed_actor_bound_operations() {
        let actor = Uuid::new_v4();
        let lead = Uuid::new_v4();
        let trace = Uuid::new_v4();
        let received = Arc::new(Mutex::new(Vec::<(String, HeaderMap, Value)>::new()));
        let recorder = received.clone();
        let app = Router::new().route(
            "/v1/agent-crm/leads",
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let recorder = recorder.clone();
                async move {
                    recorder
                        .lock()
                        .unwrap()
                        .push(("create".into(), headers, body));
                    Json(json!({"id":lead,"version":1,"traceId":trace}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let core = CoreClient {
            client: reqwest::Client::new(),
            base_url: Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap(),
            credential: "s".repeat(32),
        };
        let mut context = RequestContext {
            enterprise_user_id: actor,
            identity_binding_id: Uuid::new_v4(),
            delegation_id: Uuid::new_v4(),
            agent_id: "assistant".into(),
            agent_turn_id: "turn".into(),
            trace_id: trace,
            used_calls: 1,
            required_scope: "crm:manage".into(),
            source_buzz_event_id: "a".repeat(64),
            source_channel_id: "channel".into(),
        };
        assert!(valid_write(
            "create_crm_lead",
            &json!({"title":"仅名称即可"})
        ));
        assert!(!valid_write(
            "create_crm_lead",
            &json!({"title":"线索","actorUserId":actor})
        ));
        let first = write(
            &core,
            "create_crm_lead",
            json!({"title":"仅名称即可"}),
            &context,
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);
        context.delegation_id = Uuid::new_v4();
        let second = write(
            &core,
            "create_crm_lead",
            json!({"title":"仅名称即可"}),
            &context,
        )
        .await;
        assert_eq!(second.status(), StatusCode::OK);
        let records = received.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].1["x-enterprise-user-id"], actor.to_string());
        assert_eq!(
            records[0].1["idempotency-key"],
            records[1].1["idempotency-key"]
        );
        assert_eq!(records[0].2, json!({"title":"仅名称即可"}));
        server.abort();
    }
}
