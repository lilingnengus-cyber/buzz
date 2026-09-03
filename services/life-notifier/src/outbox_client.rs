use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Envelope {
    pub(crate) outbox_id: Uuid,
    pub(crate) lease_token: String,
    pub(crate) target: Target,
    pub(crate) category: String,
    pub(crate) sanitized_summary: String,
    pub(crate) resource_ref: ResourceRef,
    pub(crate) idempotency_key: String,
    pub(crate) trace_id: Uuid,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "lowercase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Target {
    Dm {
        community_id: String,
        pubkey: String,
    },
    Channel {
        community_id: String,
        channel_id: String,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ResourceRef {
    pub(crate) scheme: String,
    #[serde(rename = "type")]
    pub(crate) resource_type: String,
    pub(crate) id: String,
    pub(crate) version: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimResponse {
    ok: bool,
    items: Vec<Envelope>,
}

#[derive(Clone)]
pub(crate) struct OutboxClient {
    client: reqwest::Client,
    base_url: Url,
    service_token: String,
}

impl OutboxClient {
    pub(crate) fn new(base_url: Url, service_token: String) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(2))
            .timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            client,
            base_url,
            service_token,
        })
    }

    pub(crate) async fn claim(
        &self,
        lease_seconds: u64,
        community_id: &str,
    ) -> anyhow::Result<Vec<Envelope>> {
        let response = self
            .post(
                "api/internal/pacioli-outbox/claim",
                &serde_json::json!({
                    "limit": 10,
                    "leaseSeconds": lease_seconds,
                    "communityId": community_id,
                }),
            )
            .await?;
        let payload = response.json::<ClaimResponse>().await?;
        if !payload.ok {
            anyhow::bail!("LifeOS outbox claim was not successful");
        }
        Ok(payload.items)
    }

    pub(crate) async fn ack(&self, item: &Envelope, event_id: &str) -> anyhow::Result<()> {
        self.post(
            "api/internal/pacioli-outbox/ack",
            &Ack {
                outbox_id: item.outbox_id,
                lease_token: &item.lease_token,
                response_event_id: event_id,
            },
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn fail(
        &self,
        item: &Envelope,
        error_code: &'static str,
        retryable: bool,
    ) -> anyhow::Result<bool> {
        let response = self
            .post(
                "api/internal/pacioli-outbox/fail",
                &Failure {
                    outbox_id: item.outbox_id,
                    lease_token: &item.lease_token,
                    error_code,
                    retryable,
                },
            )
            .await?;
        let payload = response.json::<FailureResponse>().await?;
        if !payload.ok {
            anyhow::bail!("LifeOS outbox failure report was not successful");
        }
        let _attempts = payload.attempts;
        Ok(payload.dead_lettered)
    }

    async fn post<T: Serialize + ?Sized>(
        &self,
        path: &str,
        body: &T,
    ) -> anyhow::Result<reqwest::Response> {
        let response = self
            .client
            .post(self.base_url.join(path)?)
            .header("authorization", format!("Service {}", self.service_token))
            .json(body)
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("LifeOS outbox endpoint returned {}", response.status());
        }
        Ok(response)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Ack<'a> {
    outbox_id: Uuid,
    lease_token: &'a str,
    response_event_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Failure<'a> {
    outbox_id: Uuid,
    lease_token: &'a str,
    error_code: &'static str,
    retryable: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FailureResponse {
    ok: bool,
    dead_lettered: bool,
    attempts: i32,
}

#[cfg(test)]
mod tests {
    use super::ClaimResponse;

    #[test]
    fn claim_response_accepts_the_lifeos_api_envelope() {
        let response = serde_json::json!({
            "ok": true,
            "items": [{
                "outboxId": uuid::Uuid::new_v4(),
                "leaseToken": "l".repeat(43),
                "target": {"type": "dm", "communityId": "community", "pubkey": "a".repeat(64)},
                "category": "action_summary",
                "sanitizedSummary": "一个行动状态已更新",
                "resourceRef": {"scheme": "life", "type": "action", "id": "action-1", "version": 1},
                "idempotencyKey": format!("sha256:{}", "d".repeat(64)),
                "traceId": uuid::Uuid::new_v4(),
                "createdAt": chrono::Utc::now().to_rfc3339(),
            }]
        });
        let parsed: ClaimResponse = serde_json::from_value(response).expect("strict response");
        assert!(parsed.ok);
        assert_eq!(parsed.items.len(), 1);
    }

    #[test]
    fn claim_response_rejects_a_missing_api_success_marker() {
        assert!(serde_json::from_value::<ClaimResponse>(serde_json::json!({"items": []})).is_err());
    }
}
