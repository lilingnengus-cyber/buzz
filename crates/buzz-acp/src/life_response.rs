use crate::{acp::AcpClient, relay::RestClient, turn_observer::TurnObserver};
use life_workbench_contracts::result::{ErrorCode, LifeResourceRef, WorkbenchResult};
use nostr::Event;
use std::{any::Any, collections::HashMap, time::Duration};
use uuid::Uuid;

const MAX_RESPONSE_BYTES: usize = 128 * 1_024;
const PUBLISH_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
struct ObservedLifeResult {
    tool: String,
    succeeded: bool,
    message: String,
    resource_refs: Vec<LifeResourceRef>,
    trace_id: Uuid,
    audit_id: Option<Uuid>,
}

pub(crate) struct CapturedLifeResponse {
    text: Option<String>,
    results: Vec<ObservedLifeResult>,
    invalid_tool_result: bool,
}

#[derive(Default)]
struct LifeResponseCapture {
    tools: HashMap<String, String>,
    results: Vec<ObservedLifeResult>,
    message_id: Option<String>,
    text: String,
    too_large: bool,
    invalid_tool_result: bool,
}

pub(crate) fn start_capture(acp: &mut AcpClient) {
    acp.set_turn_observer(Some(Box::new(LifeResponseCapture::default())));
}

pub(crate) fn finish_capture(acp: &mut AcpClient) -> Option<CapturedLifeResponse> {
    let observer = acp.take_turn_observer()?;
    let capture = observer.into_any().downcast::<LifeResponseCapture>().ok()?;
    Some(capture.finish())
}

impl LifeResponseCapture {
    fn finish(mut self) -> CapturedLifeResponse {
        let text = (!self.too_large && !self.text.trim().is_empty())
            .then(|| std::mem::take(&mut self.text));
        CapturedLifeResponse {
            text,
            results: self.results,
            invalid_tool_result: self.invalid_tool_result,
        }
    }

    fn message_chunk(&mut self, update: &serde_json::Value, text: &str) {
        let Some(message_id) = update.get("messageId").and_then(|value| value.as_str()) else {
            return;
        };
        if self.message_id.as_deref() != Some(message_id) {
            self.message_id = Some(message_id.to_owned());
            self.text.clear();
            self.too_large = false;
        }
        if self.too_large {
            return;
        }
        if self.text.len().saturating_add(text.len()) > MAX_RESPONSE_BYTES {
            self.text.clear();
            self.too_large = true;
        } else {
            self.text.push_str(text);
        }
    }

    fn track_tool(&mut self, update: &serde_json::Value, title: &str) {
        let Some(tool_id) = update.get("toolCallId").and_then(|value| value.as_str()) else {
            return;
        };
        let name = title.rsplit("__").next().unwrap_or(title);
        if is_life_tool(name) {
            self.tools.insert(tool_id.to_owned(), name.to_owned());
        }
    }

    fn tool_result(&mut self, update: &serde_json::Value, tool_id: &str, status: &str) {
        let Some(tool) = self.tools.remove(tool_id) else {
            return;
        };
        if status != "completed" {
            self.results.push(ObservedLifeResult {
                tool,
                succeeded: false,
                message: "LifeOS 工具调用未完成".into(),
                resource_refs: Vec::new(),
                trace_id: Uuid::nil(),
                audit_id: None,
            });
            return;
        }
        let Some(text) = update
            .pointer("/content/0/content/text")
            .and_then(|value| value.as_str())
        else {
            self.invalid_tool_result = true;
            return;
        };
        let Ok(result) = serde_json::from_str::<WorkbenchResult<serde_json::Value>>(text) else {
            self.invalid_tool_result = true;
            return;
        };
        match result {
            WorkbenchResult::Success(success) => self.results.push(ObservedLifeResult {
                tool,
                succeeded: true,
                message: "LifeOS 已确认读取成功".into(),
                resource_refs: success.resource_refs,
                trace_id: success.trace_id,
                audit_id: Some(success.audit_id),
            }),
            WorkbenchResult::Failure(failure) => self.results.push(ObservedLifeResult {
                tool,
                succeeded: false,
                message: safe_failure_message(failure.error.code, &failure.error.message),
                resource_refs: Vec::new(),
                trace_id: failure.trace_id,
                audit_id: None,
            }),
        }
    }
}

impl TurnObserver for LifeResponseCapture {
    fn on_session_update(&mut self, update: &serde_json::Value) {
        match update.get("sessionUpdate").and_then(|value| value.as_str()) {
            Some("agent_message_chunk") => {
                if let Some(text) = update
                    .pointer("/content/text")
                    .and_then(|value| value.as_str())
                {
                    self.message_chunk(update, text);
                }
            }
            Some("tool_call") => {
                if let Some(title) = update.get("title").and_then(|value| value.as_str()) {
                    self.track_tool(update, title);
                }
            }
            Some("tool_call_update") => {
                let tool_id = update.get("toolCallId").and_then(|value| value.as_str());
                let status = update.get("status").and_then(|value| value.as_str());
                if let (Some(tool_id), Some(status)) = (tool_id, status) {
                    self.tool_result(update, tool_id, status);
                }
            }
            _ => {}
        }
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any + Send> {
        self
    }
}

fn is_life_tool(name: &str) -> bool {
    life_workbench_contracts::catalog::tool(name).is_some()
}

fn safe_failure_message(code: ErrorCode, server_message: &str) -> String {
    if (1..=2_000).contains(&server_message.chars().count())
        && server_message.trim() == server_message
        && !server_message.chars().any(char::is_control)
    {
        server_message.to_owned()
    } else {
        format!("LifeOS 请求失败：{code:?}")
    }
}

fn trusted_content(captured: CapturedLifeResponse) -> String {
    let Some(last) = captured.results.last() else {
        return if captured.invalid_tool_result {
            "LifeOS 返回了无法验证的工具结果，本次未发布原始内容。".into()
        } else {
            "本次没有获得受验证的 LifeOS 结果；请明确要读取的工作区或资源。".into()
        };
    };
    if !last.succeeded {
        return if last.trace_id.is_nil() {
            last.message.clone()
        } else {
            format!("{}\nTrace ID: {}", last.message, last.trace_id)
        };
    }
    let mut content = captured
        .text
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| last.message.clone());
    content.push_str("\n\n已验证 LifeOS 结果：");
    content.push_str(&last.tool);
    content.push_str(" succeeded");
    for reference in &last.resource_refs {
        content.push_str("\n- ");
        content.push_str(&reference.life_uri());
        if let Some(version) = reference.version() {
            content.push_str(&format!(" v{version}"));
        }
        if let Some(title) = reference.title() {
            content.push_str(" — ");
            content.push_str(title);
        }
    }
    content.push_str(&format!("\nTrace ID: {}", last.trace_id));
    if let Some(audit_id) = last.audit_id {
        content.push_str(&format!("\nAudit ID: {audit_id}"));
    }
    content
}

pub(crate) async fn publish(
    rest: &RestClient,
    channel_id: Uuid,
    source_event: &Event,
    captured: CapturedLifeResponse,
) {
    let content = trusted_content(captured);
    let parsed = crate::queue::parse_thread_tags(source_event);
    let root_id = parsed
        .root_event_id
        .as_deref()
        .and_then(|value| nostr::EventId::from_hex(value).ok())
        .unwrap_or(source_event.id);
    let thread = buzz_sdk::ThreadRef {
        root_event_id: root_id,
        parent_event_id: source_event.id,
    };
    let builder =
        match buzz_sdk::build_message(channel_id, &content, Some(&thread), &[], false, &[]) {
            Ok(builder) => builder,
            Err(error) => {
                tracing::warn!(channel = %channel_id, "Life Agent response build failed: {error}");
                return;
            }
        };
    let event = match builder.sign_with_keys(&rest.keys) {
        Ok(event) => event,
        Err(error) => {
            tracing::warn!(channel = %channel_id, "Life Agent response signing failed: {error}");
            return;
        }
    };
    let expected_id = event.id.to_hex();
    match tokio::time::timeout(PUBLISH_TIMEOUT, rest.submit_event(&event)).await {
        Ok(Ok(value))
            if value.get("accepted").and_then(|item| item.as_bool()) == Some(true)
                && value.get("event_id").and_then(|item| item.as_str())
                    == Some(expected_id.as_str()) => {}
        Ok(Ok(_)) => tracing::warn!(channel = %channel_id, "Life Agent response was not accepted"),
        Ok(Err(error)) => {
            tracing::warn!(channel = %channel_id, "Life Agent response publish failed: {error}")
        }
        Err(_) => tracing::warn!(channel = %channel_id, "Life Agent response publish timed out"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn_observer::TurnObserver;
    use serde_json::json;

    fn observe_result(capture: &mut LifeResponseCapture, text: &str) {
        capture.on_session_update(&json!({
            "sessionUpdate":"tool_call",
            "toolCallId":"tool-1",
            "title":"life-workbench-mcp__get_action_detail"
        }));
        capture.on_session_update(&json!({
            "sessionUpdate":"tool_call_update",
            "toolCallId":"tool-1",
            "status":"completed",
            "content":[{"content":{"text":text}}]
        }));
    }

    #[test]
    fn trusted_success_appends_only_server_refs_trace_and_audit() {
        let trace = Uuid::new_v4();
        let audit = Uuid::new_v4();
        let mut capture = LifeResponseCapture::default();
        capture.on_session_update(&json!({
            "sessionUpdate":"agent_message_chunk",
            "messageId":"message-1",
            "content":{"text":"你的行动仍在进行中。"}
        }));
        observe_result(
            &mut capture,
            &json!({
                "ok":true,
                "data":{"action":{"status":"DOING"}},
                "resourceRefs":[{"scheme":"life","type":"action","id":"action-1","version":8,"title":"接口设计"}],
                "auditId":audit,
                "traceId":trace
            })
            .to_string(),
        );
        let content = trusted_content(capture.finish());
        assert!(content.contains("你的行动仍在进行中"));
        assert!(content.contains("life://action/action-1 v8"));
        assert!(content.contains(&trace.to_string()));
        assert!(content.contains(&audit.to_string()));
    }

    #[test]
    fn invalid_or_failed_tool_output_never_publishes_raw_payload() {
        let mut invalid = LifeResponseCapture::default();
        observe_result(&mut invalid, "Prisma SELECT passwordHash grant-secret");
        let content = trusted_content(invalid.finish());
        assert!(!content.contains("Prisma"));
        assert!(!content.contains("grant-secret"));

        let trace = Uuid::new_v4();
        let mut failed = LifeResponseCapture::default();
        failed.on_session_update(&json!({
            "sessionUpdate":"agent_message_chunk",
            "messageId":"message-1",
            "content":{"text":"我已经成功修改了行动"}
        }));
        observe_result(
            &mut failed,
            &json!({
                "ok":false,
                "error":{"code":"scope_denied","message":"Life access was denied","retryable":false},
                "traceId":trace
            })
            .to_string(),
        );
        let content = trusted_content(failed.finish());
        assert!(!content.contains("成功修改"));
        assert!(content.contains("Life access was denied"));
        assert!(content.contains(&trace.to_string()));
    }
}
