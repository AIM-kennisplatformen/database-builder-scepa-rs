mod api;

use std::{env, error::Error, future::IntoFuture, net::SocketAddr, time::Duration};

use restate_sdk::{endpoint::Endpoint, http_server::HttpServer};
use scepa::{
    pipeline::{
        garage::{GarageClient, GaragePipelineService, PostgresPdfStore},
        grobid::HttpGrobidClient,
        typedb::TypeDbService,
    },
    postgres::PostgresReviewStore,
    restate::ScepaWorkflow,
};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use crate::api::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "scepa=debug".into()))
        .init();

    let database_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://scepa:scepa@localhost:5432/scepa".into());
    let grobid_url = env::var("GROBID_URL").unwrap_or_else(|_| "http://localhost:8070".into());
    let restate_ingress_url =
        env::var("RESTATE_INGRESS_URL").unwrap_or_else(|_| "http://localhost:8080".into());
    let restate_admin_url =
        env::var("RESTATE_ADMIN_URL").unwrap_or_else(|_| "http://localhost:9070".into());
    let typedb_address = env::var("TYPEDB_ADDRESS").unwrap_or_else(|_| "localhost:1729".into());
    let typedb_database = env::var("TYPEDB_DATABASE").unwrap_or_else(|_| "scepa".into());
    let typedb_username = env::var("TYPEDB_USERNAME").unwrap_or_else(|_| "admin".into());
    let typedb_password = env::var("TYPEDB_PASSWORD").unwrap_or_else(|_| "password".into());
    let garage_endpoint =
        env::var("GARAGE_ENDPOINT").unwrap_or_else(|_| "http://localhost:3900".into());
    let garage_region = env::var("GARAGE_REGION").unwrap_or_else(|_| "garage".into());
    let garage_bucket = env::var("GARAGE_BUCKET").unwrap_or_else(|_| "scepa-pdfs".into());
    let garage_access_key = env::var("GARAGE_ACCESS_KEY")
        .unwrap_or_else(|_| "GK00000000000000000000000000000000".into());
    let garage_secret_key = env::var("GARAGE_SECRET_KEY").unwrap_or_else(|_| {
        "0000000000000000000000000000000000000000000000000000000000000000".into()
    });
    let api_address: SocketAddr = env::var("API_ADDRESS")
        .unwrap_or_else(|_| "0.0.0.0:3000".into())
        .parse()?;
    let restate_address: SocketAddr = env::var("RESTATE_ENDPOINT_ADDRESS")
        .unwrap_or_else(|_| "0.0.0.0:9080".into())
        .parse()?;

    let review_store = PostgresReviewStore::connect_lazy(&database_url)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    review_store
        .migrate()
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let typedb = TypeDbService::connect(
        &typedb_address,
        typedb_database,
        &typedb_username,
        &typedb_password,
    )
    .await
    .map_err(|error| std::io::Error::other(error.to_string()))?;
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()?;
    let garage = GarageClient::new(
        http_client.clone(),
        &garage_endpoint,
        garage_region,
        garage_access_key,
        garage_secret_key,
    )
    .map_err(|error| std::io::Error::other(error.to_string()))?;
    let garage_pipeline = GaragePipelineService::new(
        PostgresPdfStore::new(review_store.pool().clone()),
        garage,
        garage_bucket,
        review_store.clone(),
    );

    let debug_artifact_root =
        env::var("DEBUG_ARTIFACT_ROOT").unwrap_or_else(|_| ".artifacts".into());
    let workflow = ScepaWorkflow::new(
        HttpGrobidClient::new(http_client.clone(), &grobid_url),
        review_store.clone(),
        typedb.clone(),
        garage_pipeline.clone(),
    )
    .with_debug_artifact_root(debug_artifact_root.clone());
    let api = api::router(AppState {
        review_store,
        typedb,
        garage_pipeline,
        http_client,
        restate_ingress_url,
        restate_admin_url,
        grobid_url,
        debug_artifact_root: debug_artifact_root.into(),
    });
    let api_listener = TcpListener::bind(api_address).await?;
    let api_server = axum::serve(api_listener, api).into_future();
    let restate_server = HttpServer::new(Endpoint::builder().bind(workflow).build())
        .listen_and_serve(restate_address);

    tracing::info!(%api_address, %restate_address, "starting SCEPA endpoints");
    tokio::select! {
        result = api_server => result?,
        () = restate_server => {},
    }
    Ok(())
}
