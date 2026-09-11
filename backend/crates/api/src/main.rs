mod api;

use std::{
    error::Error,
    io,
    net::SocketAddr,
    num::{NonZeroU64, NonZeroUsize},
    path::PathBuf,
    time::Duration,
};

use clap::{CommandFactory, Parser};
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

#[derive(Debug, Parser)]
#[command(
    name = "scepa-api",
    version,
    about = "SCEPA HTTP API and document pipeline"
)]
struct Config {
    /// Loads environment variables from this file instead of searching for .env.
    #[arg(long, value_name = "PATH")]
    env_file: Option<PathBuf>,

    #[arg(long, env = "API_ADDRESS", default_value = "0.0.0.0:3000")]
    api_address: SocketAddr,

    #[arg(long, env = "RESTATE_ENDPOINT_ADDRESS", default_value = "0.0.0.0:9080")]
    restate_endpoint_address: SocketAddr,

    #[arg(
        long,
        env = "RESTATE_INGRESS_URL",
        default_value = "http://localhost:8080"
    )]
    restate_ingress_url: String,

    #[arg(
        long,
        env = "RESTATE_ADMIN_URL",
        default_value = "http://localhost:9070"
    )]
    restate_admin_url: String,

    #[arg(
        long,
        env = "RESTATE_DEPLOYMENT_URL",
        default_value = "http://localhost:9080"
    )]
    restate_deployment_url: String,

    #[arg(long, env = "DATABASE_URL", hide_env_values = true)]
    database_url: String,

    #[arg(long, env = "GARAGE_ENDPOINT")]
    garage_endpoint: String,

    #[arg(long, env = "GARAGE_REGION")]
    garage_region: String,

    #[arg(long, env = "GARAGE_ACCESS_KEY", hide_env_values = true)]
    garage_access_key: String,

    #[arg(long, env = "GARAGE_SECRET_KEY", hide_env_values = true)]
    garage_secret_key: String,

    #[arg(long, env = "GARAGE_BUCKET")]
    garage_bucket: String,

    #[arg(long, env = "GROBID_URL")]
    grobid_url: String,

    #[arg(long, env = "TYPEDB_ADDRESS")]
    typedb_address: String,

    #[arg(long, env = "TYPEDB_DATABASE")]
    typedb_database: String,

    #[arg(long, env = "TYPEDB_USERNAME")]
    typedb_username: String,

    #[arg(long, env = "TYPEDB_PASSWORD", hide_env_values = true)]
    typedb_password: String,

    #[arg(
        long,
        env = "OPENAI_HOST",
        default_value = "https://api.tokenfactory.nebius.com/v1/"
    )]
    openai_host: String,

    #[arg(long, env = "OPENAI_API_KEY", hide_env_values = true)]
    openai_api_key: String,

    #[arg(
        long,
        env = "OPENAI_EMBEDDING_MODEL",
        default_value = "Qwen/Qwen3-Embedding-8B"
    )]
    openai_embedding_model: String,

    #[arg(long, env = "EMBEDDING_MAX_CONCURRENCY", default_value = "4")]
    embedding_max_concurrency: NonZeroUsize,

    #[arg(long, env = "QDRANT_URL", default_value = "http://localhost:6334")]
    qdrant_url: String,

    #[arg(long, env = "QDRANT_COLLECTION", default_value = "scepa")]
    qdrant_collection: String,

    #[arg(long, env = "QDRANT_VECTOR_SIZE", default_value = "4096")]
    qdrant_vector_size: NonZeroU64,

    #[arg(
        long,
        env = "QDRANT_API_KEY",
        hide_env_values = true,
        default_value = ""
    )]
    qdrant_api_key: String,
}

impl Config {
    fn load() -> Result<Self, dotenvy::Error> {
        let env_file = Self::command()
            .ignore_errors(true)
            .get_matches()
            .remove_one::<PathBuf>("env_file");
        let dotenv_result = match env_file {
            Some(path) => dotenvy::from_path(path),
            None => dotenvy::dotenv().map(|_| ()),
        };

        match dotenv_result {
            Ok(()) => {}
            Err(error) if error.not_found() => {}
            Err(error) => return Err(error),
        }

        Ok(Self::parse())
    }
}

#[derive(Serialize)]
struct DeploymentRegistration<'a> {
    uri: &'a str,
    force: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::load()?;

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "scepa=debug".into()))
        .init();

    let review_store =
        PostgresReviewStore::connect_lazy(&config.database_url).map_err(internal_error)?;
    review_store.migrate().await.map_err(internal_error)?;

    let http_client = reqwest::Client::new();
    let garage_client = GarageClient::new(
        http_client.clone(),
        &config.garage_endpoint,
        config.garage_region,
        config.garage_access_key,
        config.garage_secret_key,
    )
    .map_err(internal_error)?;
    let garage_pipeline = GaragePipelineService::new(
        PostgresPdfStore::new(review_store.pool().clone()),
        garage_client,
        config.garage_bucket,
        review_store.clone(),
    );
    let grobid_pipeline = GrobidExtractionService::new(
        HttpGrobidClient::new(http_client.clone(), config.grobid_url),
        review_store.clone(),
    );
    let tei_pipeline = TeiConversionService::new(review_store.clone());
    let typedb = TypeDbService::connect(
        &config.typedb_address,
        config.typedb_database,
        &config.typedb_username,
        &config.typedb_password,
    )
    .await
    .map_err(internal_error)?;
    let embedding_config = EmbeddingConfig::new(
        config.openai_host,
        config.openai_api_key,
        config.openai_embedding_model,
        config.embedding_max_concurrency.get(),
    )
    .map_err(internal_error)?;
    let qdrant_config = QdrantConfig::new(
        config.qdrant_url,
        config.qdrant_collection,
        config.qdrant_vector_size.get(),
        config.qdrant_api_key,
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
    let restate_listener = TcpListener::bind(config.restate_endpoint_address).await?;
    tokio::spawn(HttpServer::new(restate_endpoint).serve(restate_listener));

    register_restate_deployment(
        &http_client,
        &config.restate_admin_url,
        &config.restate_deployment_url,
    )
    .await?;

    let api_listener = TcpListener::bind(config.api_address).await?;
    let state = api::AppState::new(
        RestateClient::new(&config.restate_ingress_url)?,
        garage_pipeline,
        review_store,
    );
    tracing::info!(
        api_address = %config.api_address,
        restate_endpoint_address = %config.restate_endpoint_address,
        restate_ingress_url = %config.restate_ingress_url,
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

fn invalid_input(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error.to_string())
}

fn internal_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    fn required_args() -> Vec<&'static str> {
        vec![
            "scepa-api",
            "--database-url",
            "postgres://localhost/scepa",
            "--garage-endpoint",
            "http://localhost:3900",
            "--garage-region",
            "garage",
            "--garage-access-key",
            "access-key",
            "--garage-secret-key",
            "secret-key",
            "--garage-bucket",
            "scepa-pdfs",
            "--grobid-url",
            "http://localhost:8070",
            "--typedb-address",
            "localhost:1729",
            "--typedb-database",
            "scepa",
            "--typedb-username",
            "admin",
            "--typedb-password",
            "password",
            "--openai-api-key",
            "openai-key",
        ]
    }

    #[test]
    fn validates_positive_numeric_configuration() {
        let mut args = required_args();
        args.extend(["--embedding-max-concurrency", "0"]);
        let error = Config::try_parse_from(args).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::ValueValidation);
    }

    #[test]
    fn parses_configuration_into_runtime_types() {
        let mut args = required_args();
        args.extend([
            "--api-address",
            "127.0.0.1:4000",
            "--embedding-max-concurrency",
            "8",
            "--qdrant-vector-size",
            "1024",
        ]);
        let config = Config::try_parse_from(args).unwrap();

        assert_eq!(config.api_address, "127.0.0.1:4000".parse().unwrap());
        assert_eq!(config.embedding_max_concurrency.get(), 8);
        assert_eq!(config.qdrant_vector_size.get(), 1024);
    }
}
