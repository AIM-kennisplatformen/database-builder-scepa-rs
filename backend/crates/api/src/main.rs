mod api;

use std::{env, error::Error, io, net::SocketAddr, time::Duration};

use reqwest::Url;
use restate_sdk::prelude::{Endpoint, HttpServer};
use scepa::{
    pipeline::{
        embedding::{EmbeddingConfig, EmbeddingSource},
        garage::{GarageClient, GaragePipelineService, PostgresPdfStore},
        grobid::{GrobidExtractionService, HttpGrobidClient},
        qdrant::{QdrantConfig, QdrantStore},
        tei::TeiConversionService,
        typedb::TypeDbService,
        vector::DocumentVectorPipeline,
    },
    postgres::PostgresReviewStore,
    restate::{
        RestateClient,
        services::{
            ArtifactRestateService, GarageRestateService, GrobidRestateService, TeiRestateService,
            TypeDbRestateService, VectorRestateService,
        },
        workflows::{
            DocumentExtractionWorkflow, FixDocumentWorkflow, NewDocumentWorkflow,
            UpdateDocumentWorkflow,
        },
    },
};
use serde::Serialize;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[derive(Serialize)]
struct DeploymentRegistration<'a> {
    uri: &'a str,
    force: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "scepa=debug".into()))
        .init();

    let api_address = address("API_ADDRESS", "0.0.0.0:3000")?;
    let restate_endpoint_address = address("RESTATE_ENDPOINT_ADDRESS", "0.0.0.0:9080")?;
    let restate_ingress_url = value("RESTATE_INGRESS_URL", "http://localhost:8080");
    let restate_admin_url = value("RESTATE_ADMIN_URL", "http://localhost:9070");
    let restate_deployment_url = value("RESTATE_DEPLOYMENT_URL", "http://localhost:9080");

    let review_store =
        PostgresReviewStore::connect_lazy(&required("DATABASE_URL")?).map_err(internal_error)?;
    review_store.migrate().await.map_err(internal_error)?;

    let http_client = reqwest::Client::new();
    let garage_client = GarageClient::new(
        http_client.clone(),
        &required("GARAGE_ENDPOINT")?,
        required("GARAGE_REGION")?,
        required("GARAGE_ACCESS_KEY")?,
        required("GARAGE_SECRET_KEY")?,
    )
    .map_err(internal_error)?;
    let garage_pipeline = GaragePipelineService::new(
        PostgresPdfStore::new(review_store.pool().clone()),
        garage_client,
        required("GARAGE_BUCKET")?,
        review_store.clone(),
    );
    let grobid_pipeline = GrobidExtractionService::new(
        HttpGrobidClient::new(http_client.clone(), required("GROBID_URL")?),
        review_store.clone(),
    );
    let tei_pipeline = TeiConversionService::new(review_store.clone());
    let typedb = TypeDbService::connect(
        &required("TYPEDB_ADDRESS")?,
        required("TYPEDB_DATABASE")?,
        &required("TYPEDB_USERNAME")?,
        &required("TYPEDB_PASSWORD")?,
    )
    .await
    .map_err(internal_error)?;
    let embedding_config = EmbeddingConfig::new(
        value("OPENAI_HOST", "https://api.tokenfactory.nebius.com/v1/"),
        required("OPENAI_API_KEY")?,
        value("OPENAI_EMBEDDING_MODEL", "Qwen/Qwen3-Embedding-8B"),
        positive_usize("EMBEDDING_MAX_CONCURRENCY", 4)?,
    )
    .map_err(internal_error)?;
    let qdrant_config = QdrantConfig::new(
        value("QDRANT_URL", "http://localhost:6334"),
        value("QDRANT_COLLECTION", "scepa"),
        positive_u64("QDRANT_VECTOR_SIZE", 4096)?,
        value("QDRANT_API_KEY", ""),
    );
    let qdrant = QdrantStore::connect(&qdrant_config)
        .await
        .map_err(internal_error)?;
    let vectors = DocumentVectorPipeline::new(EmbeddingSource::new(embedding_config), qdrant);

    let restate_endpoint = Endpoint::builder()
        .bind(GarageRestateService::new(garage_pipeline.clone()))
        .bind(GrobidRestateService::new(
            grobid_pipeline,
            garage_pipeline.clone(),
        ))
        .bind(TeiRestateService::new(tei_pipeline))
        .bind(TypeDbRestateService::new(
            typedb.clone(),
            review_store.clone(),
        ))
        .bind(VectorRestateService::new(vectors))
        .bind(ArtifactRestateService::new(review_store.clone()))
        .bind(DocumentExtractionWorkflow)
        .bind(NewDocumentWorkflow)
        .bind(UpdateDocumentWorkflow)
        .bind(FixDocumentWorkflow)
        .build();
    let restate_listener = TcpListener::bind(restate_endpoint_address).await?;
    tokio::spawn(HttpServer::new(restate_endpoint).serve(restate_listener));

    register_restate_deployment(&http_client, &restate_admin_url, &restate_deployment_url).await?;

    let api_listener = TcpListener::bind(api_address).await?;
    let state = api::AppState::new(
        RestateClient::new(&restate_ingress_url)?,
        garage_pipeline,
        review_store,
    );
    tracing::info!(
        %api_address,
        %restate_endpoint_address,
        %restate_ingress_url,
        "starting SCEPA API and Restate endpoint"
    );
    axum::serve(api_listener, api::router(state)).await?;
    Ok(())
}

async fn register_restate_deployment(
    client: &reqwest::Client,
    admin_url: &str,
    deployment_url: &str,
) -> io::Result<()> {
    let mut url = Url::parse(admin_url).map_err(invalid_input)?;
    url.path_segments_mut()
        .map_err(|()| invalid_input("Restate admin URL cannot be a base URL"))?
        .pop_if_empty()
        .push("deployments");

    let mut last_error = String::new();
    for attempt in 1..=20 {
        match client
            .post(url.clone())
            .json(&DeploymentRegistration {
                uri: deployment_url,
                force: true,
            })
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                tracing::info!(%deployment_url, "registered Restate deployment");
                return Ok(());
            }
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                last_error = format!("Restate admin returned {status}: {body}");
            }
            Err(error) => last_error = error.to_string(),
        }

        tracing::warn!(attempt, error = %last_error, "Restate deployment registration failed; retrying");
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    Err(io::Error::other(format!(
        "could not register Restate deployment {deployment_url}: {last_error}"
    )))
}

fn value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.into())
}

fn required(name: &str) -> io::Result<String> {
    env::var(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("required environment variable {name} is not set"),
        )
    })
}

fn positive_usize(name: &str, default: usize) -> io::Result<usize> {
    let raw = env::var(name).unwrap_or_else(|_| default.to_string());
    raw.parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{name} must be a positive integer"),
            )
        })
}

fn positive_u64(name: &str, default: u64) -> io::Result<u64> {
    let raw = env::var(name).unwrap_or_else(|_| default.to_string());
    raw.parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{name} must be a positive integer"),
            )
        })
}

fn address(name: &str, default: &str) -> io::Result<SocketAddr> {
    value(name, default).parse().map_err(invalid_input)
}

fn invalid_input(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error.to_string())
}

fn internal_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
