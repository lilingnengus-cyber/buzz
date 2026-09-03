use std::net::SocketAddr;

use metrics_exporter_prometheus::PrometheusBuilder;

pub(crate) fn install(address: SocketAddr) -> anyhow::Result<()> {
    let (recorder, exporter) = PrometheusBuilder::new()
        .with_http_listener(address)
        .build()?;
    metrics::set_global_recorder(recorder)?;
    tokio::spawn(exporter);
    Ok(())
}

pub(crate) fn delivery(category: &str, result: &'static str) {
    let category = match category {
        "action_summary" => "action_summary",
        "project_status" => "project_status",
        _ => "invalid",
    };
    metrics::counter!("life_notifier_deliveries_total", "category" => category, "result" => result)
        .increment(1);
}

pub(crate) fn claimed(count: usize) {
    metrics::counter!("life_notifier_claimed_total").increment(count as u64);
}

pub(crate) fn observe_outbox_lag(seconds: f64) {
    metrics::histogram!("life_notifier_outbox_lag_seconds").record(seconds.max(0.0));
}

pub(crate) fn transition(result: &'static str) {
    debug_assert!(matches!(
        result,
        "acknowledged" | "retry" | "dead_letter" | "failure_report_failed" | "ack_failed"
    ));
    metrics::counter!("life_notifier_outbox_transitions_total", "result" => result).increment(1);
}
