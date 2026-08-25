mod embedding;
mod models;
mod qdrant;
mod reranker;
mod search;
mod typedb;

use std::{env, io, net::SocketAddr, sync::Arc};

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::wrapper::{Json, Parameters},
    schemars::JsonSchema,
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, session::local::LocalSessionManager,
        tower::StreamableHttpService,
    },
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use crate::reranker::OnnxReranker;
use crate::{
    embedding::EmbeddingClient,
    models::{LiteratureFilters, LiteratureSearchResponse, MetadataResponse},
    qdrant::PassageStore,
    search::{LiteratureSearchService, SearchError},
    typedb::MetadataStore,
};

#[derive(Clone)]
struct LiteratureMcp {
    search: Arc<LiteratureSearchService>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchLiteratureParameters {
    /// Natural-language literature query.
    query: String,
    /// Optional TypeDB-backed publication metadata filters.
    #[serde(default)]
    filters: LiteratureFilters,
    /// Include TypeDB metadata for the documents represented in the results.
    #[serde(default)]
    include_metadata: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetDocumentMetadataParameters {
    /// One or more opaque pdf_hash values returned by another SCEPA MCP tool.
    pdf_hashes: Vec<String>,
}

#[tool_router]
impl LiteratureMcp {
    #[tool(
        description = "Search the published literature. The returned pdf_hash is opaque and must only be passed to other SCEPA MCP tools such as get_document_metadata. Filters are resolved in TypeDB before source-passage similarity search; linked combined passages are then reranked."
    )]
    async fn search_literature(
        &self,
        Parameters(parameters): Parameters<SearchLiteratureParameters>,
    ) -> Result<Json<LiteratureSearchResponse>, McpError> {
        let query = parameters.query.trim();
        if query.is_empty() {
            return Err(McpError::invalid_params("query must not be empty", None));
        }
        self.search
            .search(query, &parameters.filters, parameters.include_metadata)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        description = "Retrieve TypeDB document metadata for one or more opaque pdf_hash values returned by SCEPA MCP tools. Do not present pdf_hash as a user-facing citation or document identifier."
    )]
    async fn get_document_metadata(
        &self,
        Parameters(parameters): Parameters<GetDocumentMetadataParameters>,
    ) -> Result<Json<MetadataResponse>, McpError> {
        self.search
            .metadata(&parameters.pdf_hashes)
            .await
            .map(Json)
            .map_err(tool_error)
    }
}

#[tool_handler(
    name = "scepa-literature",
    version = "0.1.0",
    instructions = "Use search_literature for evidence passages. Treat pdf_hash values as opaque handles only and pass them to get_document_metadata when bibliographic details are needed."
)]
impl ServerHandler for LiteratureMcp {}

#[derive(Clone)]
struct BearerToken([u8; 32]);

impl BearerToken {
    fn new(token: &str) -> Self {
        Self(Sha256::digest(token.as_bytes()).into())
    }

    fn matches(&self, token: &str) -> bool {
        let candidate: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        bool::from(self.0.ct_eq(&candidate))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "scepa=info,scepa_mcp=info".into()),
        )
        .init();

    let address: SocketAddr = value("MCP_ADDRESS", "0.0.0.0:8002")
        .parse()
        .map_err(invalid_input)?;
    let bearer = Arc::new(BearerToken::new(&required("MCP_BEARER_TOKEN")?));
    let typedb = MetadataStore::connect(
        &required("TYPEDB_ADDRESS")?,
        required("TYPEDB_DATABASE")?,
        &required("TYPEDB_USERNAME")?,
        &required("TYPEDB_PASSWORD")?,
    )
    .await
    .map_err(internal_error)?;
    let embeddings = EmbeddingClient::new(
        value("OPENAI_HOST", "https://api.tokenfactory.nebius.com/v1/"),
        required("OPENAI_API_KEY")?,
        value("OPENAI_EMBEDDING_MODEL", "Qwen/Qwen3-Embedding-8B"),
    );
    let qdrant = PassageStore::connect(
        &value("QDRANT_URL", "http://localhost:6334"),
        value("QDRANT_COLLECTION", "scepa"),
        positive_u64("QDRANT_VECTOR_SIZE", 4096)?,
        &value("QDRANT_API_KEY", ""),
    )
    .await
    .map_err(internal_error)?;
    let reranker = OnnxReranker::load(
        value("RERANK_MODEL_ID", "cross-encoder/ms-marco-MiniLM-L6-v2"),
        value("RERANK_MODEL_REVISION", "main"),
        positive_usize("RERANK_BATCH_SIZE", 32)?,
    )
    .await
    .map_err(internal_error)?;
    let search = Arc::new(
        LiteratureSearchService::new(
            typedb,
            qdrant,
            embeddings,
            reranker,
            positive_usize("SEARCH_RESULT_COUNT", 5)?,
        )
        .map_err(internal_error)?,
    );
    let handler = LiteratureMcp { search };
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true);
    let mcp_service = StreamableHttpService::new(
        move || Ok(handler.clone()),
        LocalSessionManager::default().into(),
        config,
    );
    let protected = Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(middleware::from_fn_with_state(bearer, authorize));
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .merge(protected);
    let listener = TcpListener::bind(address).await?;
    tracing::info!(%address, "starting SCEPA literature MCP server");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

async fn authorize(
    State(expected): State<Arc<BearerToken>>,
    headers: HeaderMap,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "));
    match token {
        Some(token) if expected.matches(token) => next.run(request).await,
        _ => StatusCode::UNAUTHORIZED.into_response(),
    }
}

fn tool_error(error: SearchError) -> McpError {
    match error {
        SearchError::InvalidInput(message) => McpError::invalid_params(message, None),
        error => McpError::internal_error(error.to_string(), None),
    }
}

fn value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.into())
}

fn required(name: &str) -> io::Result<String> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("required environment variable {name} is not set"),
            )
        })
}

fn positive_usize(name: &str, default: usize) -> io::Result<usize> {
    value(name, &default.to_string())
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid_input(format!("{name} must be a positive integer")))
}

fn positive_u64(name: &str, default: u64) -> io::Result<u64> {
    value(name, &default.to_string())
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid_input(format!("{name} must be a positive integer")))
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

    #[test]
    fn bearer_comparison_accepts_only_the_configured_token() {
        let token = BearerToken::new("secret");
        assert!(token.matches("secret"));
        assert!(!token.matches("other"));
    }
}
