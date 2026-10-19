//! Public Axum API for document ingestion and review.

mod documents;
mod drafts;
mod pdfs;
mod review;

use crate::error::{
    ApiError, DocumentValidationErrorResponse, ErrorResponse, UploadErrorResponse, api_error,
    document_upload_error, internal, normalize_error_response, restate_api_error,
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use scepa::{
    document_upload::DocumentUpload,
    models::{
        canonical::{CanonicalMissingField, CanonicalModel, canonical_missing_fields},
        draft::{DraftDocument, ManualDocument},
    },
    pipeline::garage::{GaragePipelineService, sha256_hex},
    postgres::{
        PostgresReviewStore, PublishedDocument, PublishedDocumentSummary, ReviewCaseDocumentSummary,
    },
    restate::{
        RestateClient,
        services::RepairDraft,
        workflows::{
            FixDocumentWorkflowRequest, NewDocumentWorkflowResponse, UpdateDocumentWorkflowResponse,
        },
    },
};
use serde::{Deserialize, Serialize};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

const MAX_PDF_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub(crate) uploads: DocumentUpload,
    pub(crate) restate: RestateClient,
    pub(crate) pdfs: GaragePipelineService,
    pub(crate) drafts: PostgresReviewStore,
}

impl AppState {
    pub fn new(
        restate: RestateClient,
        pdfs: GaragePipelineService,
        drafts: PostgresReviewStore,
    ) -> Self {
        Self {
            uploads: DocumentUpload::new(pdfs.clone(), restate.clone()),
            restate,
            pdfs,
            drafts,
        }
    }
}

#[derive(Serialize, ToSchema)]
struct UploadResponse {
    workflow_id: String,
    result: NewDocumentWorkflowResponse,
}

#[derive(Serialize, ToSchema)]
struct SubmissionResponse {
    workflow_id: String,
}

#[derive(Serialize, ToSchema)]
struct DraftResponse {
    pdf_hash: String,
    #[serde(flatten)]
    draft: DraftDocument,
}

#[derive(Serialize, ToSchema)]
struct PublishResponse {
    artifact: DraftResponse,
    canonical: CanonicalModel,
}

#[derive(Serialize, ToSchema)]
struct RepairDraftResponse {
    case: scepa::postgres::ReviewCase,
    draft: DraftResponse,
    missing_fields: Vec<CanonicalMissingField>,
}

#[derive(Deserialize, ToSchema)]
struct RepairDocumentRequest {
    manual_data: ManualDocument,
    #[serde(default)]
    enrich: bool,
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "SCEPA API",
        description = "Document ingestion, review, and publication API"
    ),
    paths(
        pdfs::upload_pdf::upload_pdf,
        pdfs::download_pdf::download_pdf,
        pdfs::submit_pdf::submit_pdf,
        drafts::get_draft::get_draft,
        drafts::publish_draft::publish_draft,
        documents::list_documents::list_documents,
        review::list_documents_requiring_fixing::list_documents_requiring_fixing,
        review::get_document_requiring_fixing::get_document_requiring_fixing,
        review::fix_document::fix_document,
        documents::get_published_document::get_published_document,
        documents::update_document::update_document
    ),
    tags(
        (name = "documents", description = "Document ingestion and publication"),
        (name = "review", description = "Operator review and repair")
    )
)]
struct ApiDoc;

/// Builds the document-upload HTTP router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/openapi.json", openapi()))
        .route("/pdfs", post(pdfs::upload_pdf::upload_pdf))
        .route("/pdfs/{pdf_hash}", get(pdfs::download_pdf::download_pdf))
        .route(
            "/pdfs/submissions/{workflow_id}",
            post(pdfs::submit_pdf::submit_pdf),
        )
        .route(
            "/drafts/{pdf_hash}",
            get(drafts::get_draft::get_draft).put(drafts::publish_draft::publish_draft),
        )
        .route("/documents", get(documents::list_documents::list_documents))
        .route(
            "/documents/requiring-fixing",
            get(review::list_documents_requiring_fixing::list_documents_requiring_fixing),
        )
        .route(
            "/documents/requiring-fixing/{case_id}",
            get(review::get_document_requiring_fixing::get_document_requiring_fixing)
                .put(review::fix_document::fix_document),
        )
        .route(
            "/documents/{pdf_hash}",
            get(documents::get_published_document::get_published_document)
                .put(documents::update_document::update_document),
        )
        .layer(DefaultBodyLimit::max(MAX_PDF_BYTES))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn(normalize_error_response))
        .with_state(state)
}

/// Generates the OpenAPI 3.1 document for the public HTTP API.
pub fn openapi() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

async fn validate_manual_update(
    state: &AppState,
    pdf_hash: &str,
    manual_data: &ManualDocument,
) -> Result<(), ApiError> {
    let artifact = if let Some(published) = state
        .drafts
        .get_published_document(pdf_hash)
        .await
        .map_err(internal)?
    {
        published.artifact
    } else {
        state
            .drafts
            .get_draft_document(pdf_hash)
            .await
            .map_err(internal)?
            .ok_or_else(|| {
                api_error(
                    StatusCode::NOT_FOUND,
                    "Document artifact not found",
                    "Upload the document again or refresh the document list",
                )
                .with_pdf_hash(pdf_hash.to_owned())
            })?
    };
    validate_artifact_update(artifact, manual_data)
}

fn validate_artifact_update(
    mut artifact: DraftDocument,
    manual_data: &ManualDocument,
) -> Result<(), ApiError> {
    manual_data.classification.validate().map_err(|error| {
        api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            error,
            "Correct the document classification and try again",
        )
    })?;
    artifact.manual_data = manual_data.clone();
    artifact.validate_ids().map_err(|error| {
        api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            error,
            "Correct the document identities and try again",
        )
    })
}

fn assign_request_ids(
    pdf_hash: &str,
    headers: &HeaderMap,
    manual_data: &mut ManualDocument,
) -> Result<(), ApiError> {
    if !manual_data.requires_identity_assignment() {
        return Ok(());
    }
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            api_error(
                StatusCode::BAD_REQUEST,
                "Idempotency-Key is required when creating entities",
                "Add a non-empty Idempotency-Key header and try again",
            )
        })?;
    manual_data.assign_missing_ids(pdf_hash, idempotency_key);
    Ok(())
}

async fn load_repair(drafts: &PostgresReviewStore, case_id: i64) -> Result<RepairDraft, ApiError> {
    let case = drafts
        .get_case(case_id)
        .await
        .map_err(internal)?
        .filter(|case| case.status == "pending")
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "Pending review case not found",
                "Refresh the review list and choose a pending case",
            )
            .with_review_case_id(case_id)
        })?;
    let pdf_hash = case.pdf_hash.clone().ok_or_else(|| {
        api_error(
            StatusCode::CONFLICT,
            "Review case is not linked to a source PDF",
            "Contact support with the review-case identifier",
        )
        .with_review_case_id(case_id)
    })?;
    let draft = drafts.get_repair_draft(&pdf_hash).await.map_err(internal)?;
    Ok(RepairDraft {
        case,
        pdf_hash,
        draft,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn openapi_contains_every_public_operation() {
        let document = serde_json::to_value(openapi()).unwrap();
        let paths = document["paths"].as_object().unwrap();
        for documentation_path in ["/openapi.json", "/swagger-ui"] {
            assert!(
                !paths.contains_key(documentation_path),
                "documentation endpoint {documentation_path} should not document itself"
            );
        }
        let expected = [
            ("/pdfs", "post"),
            ("/pdfs/{pdf_hash}", "get"),
            ("/pdfs/submissions/{workflow_id}", "post"),
            ("/drafts/{pdf_hash}", "get"),
            ("/drafts/{pdf_hash}", "put"),
            ("/documents", "get"),
            ("/documents/requiring-fixing", "get"),
            ("/documents/requiring-fixing/{case_id}", "get"),
            ("/documents/requiring-fixing/{case_id}", "put"),
            ("/documents/{pdf_hash}", "get"),
            ("/documents/{pdf_hash}", "put"),
        ];

        for (path, method) in expected {
            assert!(
                paths.get(path).and_then(|item| item.get(method)).is_some(),
                "missing {method} {path} from OpenAPI document"
            );
        }
    }

    #[test]
    fn openapi_contains_structured_wire_schemas() {
        let document = serde_json::to_value(openapi()).unwrap();
        let schemas = document["components"]["schemas"].as_object().unwrap();

        for (name, property) in [
            ("ManualDocument", "bibliography"),
            ("ManualDocument", "classification"),
            ("DocumentClassification", "user_personas"),
            ("DocumentClassification", "literature_kind"),
            ("DraftDocument", "grobid_extraction_data"),
            ("ManualDocument", "description"),
            ("TeiDocument", "id"),
            ("Contributor", "id"),
            ("Contributor", "contribution_id"),
            ("DraftAffiliation", "id"),
            ("DraftOrganization", "id"),
            ("DraftPublicationVenue", "id"),
            ("Authorship", "contribution_id"),
            ("Affiliation", "affiliation_id"),
            ("Publication", "publication_event_id"),
            ("ReviewCase", "workflow_id"),
            ("CanonicalModel", "publication_events"),
            ("NewDocumentWorkflowResponse", "stored_pdf"),
            ("UpdateDocumentWorkflowResponse", "changes"),
            ("Document", "doi"),
            ("Document", "isbn"),
            ("ProjectReport", "classification"),
            ("ProjectReport", "description"),
            ("CanonicalUpdateSummary", "contributors_inserted"),
            ("CanonicalMissingField", "path"),
            ("DocumentValidationErrorResponse", "missing_fields"),
        ] {
            assert!(
                schemas[name]["properties"].get(property).is_some(),
                "{name} is missing its {property} property: {}",
                schemas[name]
            );
        }

        assert_eq!(
            schemas["UserPersona"]["enum"],
            serde_json::json!(["strategic_overview", "best_practices", "target_groups"])
        );
        assert_eq!(
            schemas["LiteratureKind"]["enum"],
            serde_json::json!(["grey_literature", "scientific_literature", "project_report"])
        );
        let personas = &schemas["DocumentClassification"]["properties"]["user_personas"];
        assert_eq!(personas["maxItems"], 3);
        assert_eq!(personas["uniqueItems"], true);

        let published_summary = &schemas["PublishedDocumentSummary"]["properties"];
        for field in ["pdf_hash", "title", "published_at"] {
            assert!(
                published_summary.get(field).is_some(),
                "published summary is missing {field}: {published_summary}"
            );
        }
        assert!(published_summary.get("identifiers").is_none());

        let fixing_summary = &schemas["ReviewCaseDocumentSummary"]["allOf"];
        assert_eq!(fixing_summary[0]["$ref"], "#/components/schemas/ReviewCase");
        for field in ["title", "published_at"] {
            assert!(
                fixing_summary[1]["properties"].get(field).is_some(),
                "fixing summary is missing {field}: {fixing_summary}"
            );
        }
        assert!(fixing_summary[1]["properties"].get("identifiers").is_none());

        for path in [
            "/drafts/{pdf_hash}",
            "/documents/requiring-fixing/{case_id}",
            "/documents/{pdf_hash}",
        ] {
            assert!(
                document["paths"][path]["put"]["parameters"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|parameter| parameter["name"] == "Idempotency-Key"),
                "{path} must document Idempotency-Key"
            );
        }

        assert!(
            schemas["EDocument"]["oneOf"].is_array(),
            "canonical document variants must be represented as a union"
        );

        let list_items = &document["paths"]["/documents"]["get"]["responses"]["200"]["content"]["application/json"]
            ["schema"]["items"];
        assert!(
            list_items.get("$ref").is_some(),
            "document list items must reference their concrete schema: {list_items}"
        );

        assert!(
            schemas["ErrorResponse"]["properties"]
                .get("error")
                .is_some(),
            "the API error schema must expose a human-readable error message"
        );
        for property in ["action", "workflow_id", "pdf_hash", "review_case_id"] {
            assert!(
                schemas["ErrorResponse"]["properties"]
                    .get(property)
                    .is_some(),
                "the API error schema must expose {property}"
            );
        }
        assert_eq!(
            document["paths"]["/pdfs"]["post"]["responses"]["422"]["content"]["application/json"]["schema"]
                ["$ref"],
            "#/components/schemas/UploadErrorResponse"
        );

        for (path, method) in [
            ("/pdfs", "post"),
            ("/pdfs/submissions/{workflow_id}", "post"),
            ("/drafts/{pdf_hash}", "put"),
            ("/documents/requiring-fixing/{case_id}", "put"),
            ("/documents/{pdf_hash}", "put"),
        ] {
            assert!(
                document["paths"][path][method]["responses"]
                    .get("413")
                    .is_some(),
                "body-accepting operation {method} {path} must document its size-limit response"
            );
        }

        for path in document["paths"].as_object().unwrap().values() {
            for operation in path.as_object().unwrap().values() {
                let Some(responses) = operation.get("responses").and_then(Value::as_object) else {
                    continue;
                };
                for (status, response) in responses {
                    if status.starts_with('2') {
                        continue;
                    }
                    let content = &response["content"];
                    assert!(
                        content.get("application/json").is_some(),
                        "error response {status} is not documented as JSON: {content}"
                    );
                    let schema = content["application/json"]["schema"]["$ref"]
                        .as_str()
                        .unwrap_or_default();
                    assert!(
                        matches!(
                            schema,
                            "#/components/schemas/ErrorResponse"
                                | "#/components/schemas/DocumentValidationErrorResponse"
                                | "#/components/schemas/UploadErrorResponse"
                        ),
                        "error response {status} uses an unexpected schema: {schema}"
                    );
                }
            }
        }
    }

    #[test]
    fn new_manual_entities_require_an_idempotency_key_and_receive_stable_ids() {
        let value = json!({
            "bibliography": {
                "authors": [{
                    "name": "Ada Lovelace",
                    "forename": "Ada",
                    "surname": "Lovelace",
                    "affiliation": {
                        "organization": { "name": "Example University", "ror_id": null }
                    },
                    "role": "author"
                }]
            }
        });
        let mut without_key: ManualDocument = serde_json::from_value(value.clone()).unwrap();
        let error =
            assign_request_ids(&"a".repeat(64), &HeaderMap::new(), &mut without_key).unwrap_err();
        assert_eq!(error.0, StatusCode::BAD_REQUEST);

        let mut headers = HeaderMap::new();
        headers.insert("idempotency-key", HeaderValue::from_static("save-1"));
        let mut first: ManualDocument = serde_json::from_value(value.clone()).unwrap();
        let mut retry: ManualDocument = serde_json::from_value(value).unwrap();
        assign_request_ids(&"a".repeat(64), &headers, &mut first).unwrap();
        assign_request_ids(&"a".repeat(64), &headers, &mut retry).unwrap();
        assert_eq!(first, retry);
        let author = &first.bibliography.authors.as_ref().unwrap()[0];
        assert!(!author.id.is_empty());
        assert!(!author.contribution_id.is_empty());
        assert!(!author.affiliation.as_ref().unwrap().id.is_empty());
    }
}
