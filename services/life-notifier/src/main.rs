mod config;
mod message;
mod metrics;
mod outbox_client;
mod publisher;

use config::Config;
use outbox_client::OutboxClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    if !config::enabled_from_env()? {
        tracing::info!(enabled = false, "life notifier disabled");
        return Ok(());
    }
    let config = Config::from_env()?;
    metrics::install(config.metrics_bind_addr)?;
    tracing::info!(enabled = config.enabled, "life notifier configured");
    let client = OutboxClient::new(config.lifeos_base_url.clone(), config.service_token.clone())?;
    loop {
        match client
            .claim(config.lease_seconds, &config.community_id)
            .await
        {
            Ok(items) => {
                metrics::claimed(items.len());
                for item in items {
                    metrics::observe_outbox_lag(
                        (chrono::Utc::now() - item.created_at)
                            .to_std()
                            .unwrap_or_default()
                            .as_secs_f64(),
                    );
                    let target_community = match &item.target {
                        outbox_client::Target::Dm { community_id, .. }
                        | outbox_client::Target::Channel { community_id, .. } => community_id,
                    };
                    if target_community != &config.community_id || message::validate(&item).is_err()
                    {
                        metrics::delivery(&item.category, "failure");
                        tracing::warn!(outbox_id = %item.outbox_id, trace_id = %item.trace_id, error_class = "invalid_envelope", "notification envelope rejected");
                        match client.fail(&item, "invalid_envelope", false).await {
                            Ok(true) => metrics::transition("dead_letter"),
                            Ok(false) => metrics::transition("retry"),
                            Err(_) => metrics::transition("failure_report_failed"),
                        }
                        continue;
                    }
                    match publisher::publish(config.relay_url.as_str(), &config.keys, &item).await {
                        Ok(event_id) => {
                            metrics::delivery(&item.category, "success");
                            if let Err(error) = client.ack(&item, &event_id).await {
                                metrics::transition("ack_failed");
                                tracing::warn!(outbox_id = %item.outbox_id, trace_id = %item.trace_id, error_class = "ack_failed", "notification acknowledgement failed");
                                let _ = error;
                            } else {
                                metrics::transition("acknowledged");
                            }
                        }
                        Err(error) => {
                            metrics::delivery(&item.category, "failure");
                            tracing::warn!(outbox_id = %item.outbox_id, trace_id = %item.trace_id, error_class = "publish_failed", "notification publication failed");
                            let _ = error;
                            match client.fail(&item, "relay_publish_failed", true).await {
                                Ok(true) => metrics::transition("dead_letter"),
                                Ok(false) => metrics::transition("retry"),
                                Err(_) => metrics::transition("failure_report_failed"),
                            }
                        }
                    }
                }
            }
            Err(error) => {
                tracing::warn!(error_class = "claim_failed", "notification claim failed");
                let _ = error;
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(config.poll_interval) => {},
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    Ok(())
}
