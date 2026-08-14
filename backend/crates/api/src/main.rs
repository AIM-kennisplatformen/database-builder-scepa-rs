mod api;

use std::{env, error::Error, net::SocketAddr};

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "scepa=debug".into()))
        .init();

    let api_address: SocketAddr = env::var("API_ADDRESS")
        .unwrap_or_else(|_| "0.0.0.0:3000".into())
        .parse()?;
    let api_listener = TcpListener::bind(api_address).await?;

    tracing::info!(%api_address, "starting SCEPA API");
    axum::serve(api_listener, api::router()).await?;
    Ok(())
}
