use super::*;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};
pub(super) struct Client {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next: u64,
}
impl Client {
    pub(super) async fn start(
        binary: &str,
        gateway: &Url,
        api: &Url,
        credential: &str,
        issued: &Value,
        scope: Option<&str>,
    ) -> Self {
        let mut command = Command::new(binary);
        command
            .env("BUSINESS_READ_ADAPTER", "production")
            .env("BUSINESS_AUTH_GATEWAY_BASE_URL", gateway.as_str())
            .env("BUSINESS_READ_API_BASE_URL", api.as_str())
            .env("BUSINESS_READ_SERVICE_CREDENTIAL", credential)
            .env(
                "BUSINESS_AGENT_DELEGATION_TOKEN",
                issued["token"].as_str().unwrap(),
            )
            .env(
                "BUSINESS_AGENT_TRACE_ID",
                issued["traceId"].as_str().unwrap(),
            )
            .env("BUSINESS_AGENT_ID", "adjustment-chain-agent")
            .env("BUSINESS_AGENT_TURN_ID", issued["turnId"].as_str().unwrap())
            .env("BUSINESS_AGENT_DRAFT_WRITE_ENABLED", "true")
            .env("BUSINESS_CHAT_APPROVAL_ENABLED", "true")
            .env("BUSINESS_TOOL_MAX_PAYLOAD_BYTES", "131072")
            .env_remove("BUSINESS_AGENT_APPROVAL_SCOPE")
            .env("BUSINESS_ACTION_ENABLED", "false")
            .kill_on_drop(true)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        if let Some(scope) = scope {
            command.env("BUSINESS_AGENT_APPROVAL_SCOPE", scope);
        }
        let mut child = command.spawn().unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut client = Self {
            child,
            input,
            output,
            next: 1,
        };
        let init=client.request("initialize",json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"adjustment-chain","version":"1"}})).await;
        assert!(init.get("result").is_some(), "{init}");
        client
            .send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
        client
    }
    async fn send(&mut self, value: Value) {
        self.input
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
        self.input.flush().await.unwrap();
    }
    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let mut line = String::new();
                assert!(
                    self.output.read_line(&mut line).await.unwrap() > 0,
                    "MCP terminated"
                );
                let value: Value = serde_json::from_str(&line).unwrap();
                if value["id"] == id {
                    return value;
                }
            }
        })
        .await
        .expect("bounded MCP response")
    }
    pub(super) async fn call(&mut self, tool: &str, input: Value) -> Value {
        let response = self
            .request("tools/call", json!({"name":tool,"arguments":input}))
            .await;
        let content = response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("Unexpected MCP response: {response}"));
        serde_json::from_str(content).unwrap()
    }
    pub(super) async fn stop(mut self) {
        self.child.kill().await.unwrap();
        self.child.wait().await.unwrap();
    }
}
