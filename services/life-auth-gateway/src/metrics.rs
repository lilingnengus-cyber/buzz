//! Low-cardinality Life authorization metrics.

use std::{error::Error, net::SocketAddr};

use metrics_exporter_prometheus::PrometheusBuilder;

/// Installs a private Prometheus listener for this standalone process.
pub fn install(address: SocketAddr) -> Result<(), Box<dyn Error + Send + Sync>> {
    let (recorder, exporter) = PrometheusBuilder::new()
        .with_http_listener(address)
        .build()?;
    metrics::set_global_recorder(recorder)?;
    tokio::spawn(exporter);
    Ok(())
}

/// Records a Gateway decision using fixed labels only.
pub fn decision(operation: &'static str, result: &'static str) {
    debug_assert!(matches!(
        operation,
        "delegate_issue" | "delegate_consume" | "disclosure" | "target_selection"
    ));
    debug_assert!(matches!(
        result,
        "success" | "denied" | "conflict" | "failure"
    ));
    metrics::counter!("life_gateway_decisions_total", "operation" => operation, "result" => result)
        .increment(1);
}

/// Records active delegation gauge without identity labels.
pub fn active_delegations(value: f64) {
    metrics::gauge!("life_gateway_active_delegations").set(value);
}
