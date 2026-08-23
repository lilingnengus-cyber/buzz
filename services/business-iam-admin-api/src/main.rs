use business_iam_admin_api::{auth::Authenticator, router, AppState, Config, Store};
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let command = std::env::args().nth(1);
    if command.as_deref() == Some("--version") {
        println!("business-iam-admin-api {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if command.as_deref() == Some("--migrate-only") {
        let database_url = std::env::var("BUSINESS_IAM_ADMIN_DATABASE_URL")?;
        let runtime_role = std::env::var("BUSINESS_IAM_ADMIN_RUNTIME_DATABASE_ROLE")?;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await?;
        Store::migrate(&pool).await?;
        Store::grant_runtime(&pool, &runtime_role).await?;
        return Ok(());
    }
    let config = Config::from_env().map_err(|error| format!("configuration error: {error}"))?;
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;
    let store = Store::new(pool);
    store.ready().await?;
    let state = AppState {
        store,
        authenticator: Authenticator::new(&config),
        config: config.clone(),
    };
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    tracing::info!(address=%config.bind_addr,"Business IAM admin API listening");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}
