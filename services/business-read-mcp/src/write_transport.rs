use super::*;

impl BusinessReadMcp {
    pub(super) async fn call_write_api<T: Serialize>(
        &self,
        tool: &str,
        input: &T,
        context: &DelegationContext,
    ) -> Result<Value, BusinessCallError> {
        let base = self
            .config
            .business_api_base_url
            .as_ref()
            .ok_or(BusinessCallError::Unavailable)?;
        let url = base
            .join(&format!("v1/write/{tool}"))
            .map_err(|_| BusinessCallError::Unavailable)?;
        for attempt in 0..2 {
            let response = self
                .client
                .post(url.clone())
                .header(
                    "x-business-service-credential",
                    &self.config.service_credential,
                )
                .header("x-business-service-audience", &self.config.service_audience)
                .header(
                    "x-enterprise-user-id",
                    context.enterprise_user_id.to_string(),
                )
                .header(
                    "x-identity-binding-id",
                    context.identity_binding_id.to_string(),
                )
                .header("x-agent-delegation-id", context.delegation_id.to_string())
                .header("x-agent-id", &context.agent_id)
                .header("x-agent-turn-id", &context.agent_turn_id)
                .header("x-agent-used-calls", context.used_calls.to_string())
                .header("x-agent-required-scope", &context.required_scope)
                .header("x-source-buzz-event-id", &context.source_buzz_event_id)
                .header("x-source-channel-id", &context.source_channel_id)
                .header("x-trace-id", context.trace_id.to_string())
                .json(input)
                .send()
                .await;
            let response = match response {
                Ok(value) => value,
                Err(_) if attempt == 0 => continue,
                Err(_) => return Err(BusinessCallError::Unavailable),
            };
            if response.status().is_server_error() && attempt == 0 {
                continue;
            }
            if tool == "create_customer" && response.status().as_u16() == 409 {
                let value: Value = bounded_json(response, self.config.max_payload_bytes)
                    .await
                    .map_err(|_| BusinessCallError::Unavailable)?;
                if value.get("code").and_then(Value::as_str)
                    == Some("duplicate_confirmation_required")
                    && value.get("traceId").and_then(Value::as_str)
                        == Some(context.trace_id.to_string().as_str())
                {
                    if let Some(message) = value.get("message").and_then(Value::as_str) {
                        return Ok(
                            json!({"schemaVersion":1,"status":"confirmation_required","message":message,"traceId":context.trace_id,"resourceRefs":[]}),
                        );
                    }
                }
                return Err(BusinessCallError::Unavailable);
            }
            if !response.status().is_success() {
                return Err(match response.status().as_u16() {
                    403 | 404 => BusinessCallError::NotFoundOrForbidden,
                    429 => BusinessCallError::RateLimited,
                    _ => BusinessCallError::Unavailable,
                });
            }
            return bounded_json(response, self.config.max_payload_bytes)
                .await
                .map_err(|_| BusinessCallError::Unavailable);
        }
        Err(BusinessCallError::Unavailable)
    }
}
