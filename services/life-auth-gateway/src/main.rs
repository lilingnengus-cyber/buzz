use life_auth_gateway::Config;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("life-auth-gateway {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let config = Config::from_env().map_err(|error| format!("configuration error: {error}"))?;
    life_auth_gateway::metrics::install(config.metrics_bind_addr())?;
    life_auth_gateway::run(config).await
}
