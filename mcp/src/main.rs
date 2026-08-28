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
    models::{
        DocumentTypeFilter, LiteratureFilters, LiteratureSearchResponse, OrganizationFilter,
        OrganizationRoleFilter, OrganizationTypeFilter, PublicationDateFilter,
    },
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
    /// Natural-language question or topic used for semantic passage retrieval.
    query: String,
    /// Maximum number of reranked passages to return. Defaults to 30. Valid values are 1 through 50.
    #[serde(default = "default_top_k")]
    #[schemars(range(min = 1, max = 50))]
    top_k: usize,
    /// Inclusive publication start date in YYYY-MM-DD format. Omit it unless the user requested a lower date bound.
    publication_date_from: Option<chrono::NaiveDate>,
    /// Inclusive publication end date in YYYY-MM-DD format. Omit it unless the user requested an upper date bound.
    publication_date_to: Option<chrono::NaiveDate>,
    /// Exact document types to include: document (base type only), research_paper, report, or book. Values are ORed; omit or pass an empty list for no document-type restriction.
    #[serde(default)]
    document_types: Vec<DocumentTypeFilter>,
    /// Organization-name substrings associated with a document. Values are ORed; omit or pass an empty list for no name restriction.
    #[serde(default)]
    organization_names: Vec<String>,
    /// How an organization is related to a document: publisher, affiliation, contributor, or any. Values are ORed; omitted or empty means any role.
    #[serde(default)]
    organization_roles: Vec<OrganizationRoleFilter>,
    /// Organization types to include: organization, institution, government_institution, educational_institution, nonprofit_institution, or publisher. Parent types include their subtypes; values are ORed. Omit or pass an empty list for no type restriction.
    #[serde(default)]
    organization_types: Vec<OrganizationTypeFilter>,
}

const DEFAULT_TOP_K: usize = 30;

fn default_top_k() -> usize {
    DEFAULT_TOP_K
}

impl SearchLiteratureParameters {
    fn filters(&self) -> LiteratureFilters {
        let publication_date = (self.publication_date_from.is_some()
            || self.publication_date_to.is_some())
        .then_some(PublicationDateFilter {
            from: self.publication_date_from,
            to: self.publication_date_to,
        });
        let organization = (!self.organization_names.is_empty()
            || !self.organization_roles.is_empty()
            || !self.organization_types.is_empty())
        .then(|| OrganizationFilter {
            names: self.organization_names.clone(),
            roles: self.organization_roles.clone(),
            types: self.organization_types.clone(),
        });
        LiteratureFilters {
            publication_date,
            document_types: self.document_types.clone(),
            organization,
        }
    }
}

#[tool_router]
impl LiteratureMcp {
    #[tool(description = r#"
        Use case:
        Search published literature for relevant evidence passages and citation metadata.

        Input arguments:
            query is the natural-language search question or topic.
            top_k is the result limit from 1 through 50 and defaults to 30.
            publication_date_from is an optional inclusive YYYY-MM-DD lower bound.
            publication_date_to is an optional inclusive YYYY-MM-DD upper bound.
            document_types accepts document, research_paper, report, and book.
            organization_names accepts organization-name substrings.
            organization_roles accepts any, publisher, affiliation, and contributor.
            organization_types accepts organization, institution, government_institution, educational_institution, nonprofit_institution, and publisher.
            Filter categories use AND while values within a category use OR.
            Only set publication or organization filters when the user requests them.

        Output arguments:
            results contains reranked passage text, an opaque pdf_hash, and an internal score from 0.0 through 1.0.
            metadata_by_pdf_hash contains bibliographic metadata and ieee_reference citations keyed by pdf_hash.
            usage_note explains restrictions on internal fields.
            Copy ieee_reference verbatim; scores and pdf_hash values must never be shown to the user.
        "#)]
    async fn search_literature(
        &self,
        Parameters(parameters): Parameters<SearchLiteratureParameters>,
    ) -> Result<Json<LiteratureSearchResponse>, McpError> {
        let query = parameters.query.trim();
        if query.is_empty() {
            return Err(McpError::invalid_params("query must not be empty", None));
        }
        let filters = parameters.filters();
        self.search
            .search(query, &filters, parameters.top_k)
            .await
            .map(Json)
            .map_err(tool_error)
    }
}

#[tool_handler(
    name = "scepa-literature",
    version = "0.1.0",
    instructions = r#"
    Use search_literature to retrieve evidence passages and their bibliographic metadata.
    Copy ieee_reference verbatim for citations.
    Treat pdf_hash values only as opaque keys that associate passages with metadata; never present them as citations or document identifiers.
    Use scores only to assess the relative relevance of returned passages; never show scores to the user.
    "#
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
    let search = Arc::new(LiteratureSearchService::new(
        typedb, qdrant, embeddings, reranker,
    ));
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

    #[test]
    fn flat_search_parameters_build_nested_service_filters() {
        let parameters = SearchLiteratureParameters {
            query: "energy poverty".into(),
            top_k: DEFAULT_TOP_K,
            publication_date_from: chrono::NaiveDate::from_ymd_opt(2020, 1, 1),
            publication_date_to: None,
            document_types: vec![DocumentTypeFilter::ResearchPaper],
            organization_names: vec!["Example University".into()],
            organization_roles: vec![OrganizationRoleFilter::Affiliation],
            organization_types: vec![OrganizationTypeFilter::EducationalInstitution],
        };

        assert_eq!(
            parameters.filters(),
            LiteratureFilters {
                publication_date: Some(PublicationDateFilter {
                    from: chrono::NaiveDate::from_ymd_opt(2020, 1, 1),
                    to: None,
                }),
                document_types: vec![DocumentTypeFilter::ResearchPaper],
                organization: Some(OrganizationFilter {
                    names: vec!["Example University".into()],
                    roles: vec![OrganizationRoleFilter::Affiliation],
                    types: vec![OrganizationTypeFilter::EducationalInstitution],
                }),
            }
        );
    }

    #[test]
    fn search_parameter_schema_documents_top_k_and_has_no_metadata_toggle() {
        let schema = serde_json::to_value(schemars::schema_for!(SearchLiteratureParameters))
            .expect("schema should serialize");
        let properties = schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .expect("schema should have properties");

        assert!(!properties.contains_key("include_metadata"));
        for parameter in [
            "query",
            "top_k",
            "publication_date_from",
            "publication_date_to",
            "document_types",
            "organization_names",
            "organization_roles",
            "organization_types",
        ] {
            assert!(
                properties
                    .get(parameter)
                    .and_then(|property| property.get("description"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|description| !description.is_empty()),
                "{parameter} should have a description"
            );
        }
        let top_k = properties.get("top_k").expect("top_k should be exposed");
        assert_eq!(top_k.get("default"), Some(&serde_json::json!(30)));
        assert_eq!(top_k.get("minimum"), Some(&serde_json::json!(1)));
        assert_eq!(top_k.get("maximum"), Some(&serde_json::json!(50)));
        assert!(
            top_k
                .get("description")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|description| description.contains("reranked passages"))
        );
    }

    #[test]
    fn omitted_top_k_deserializes_to_thirty() {
        let parameters: SearchLiteratureParameters =
            serde_json::from_value(serde_json::json!({ "query": "energy poverty" }))
                .expect("minimal search parameters should deserialize");

        assert_eq!(parameters.top_k, DEFAULT_TOP_K);
    }

    #[test]
    fn top_k_accepts_only_the_documented_range() {
        assert!(crate::search::validate_top_k(1).is_ok());
        assert!(crate::search::validate_top_k(50).is_ok());
        assert!(crate::search::validate_top_k(0).is_err());
        assert!(crate::search::validate_top_k(51).is_err());
    }

    #[test]
    fn router_exposes_only_the_documented_literature_search_tool() {
        let tools = LiteratureMcp::tool_router().list_all();

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "search_literature");
        let description = tools[0]
            .description
            .as_deref()
            .expect("search tool should have a description");
        assert!(description.contains("Use case:"));
        assert!(description.contains("Input arguments:"));
        assert!(description.contains("Output arguments:"));
        assert!(description.contains("score"));
        assert!(description.contains("must never be shown to the user"));
    }
}
