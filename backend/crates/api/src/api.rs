//! Public Axum API for uploading new documents.

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use scepa::{
    document_upload::DocumentUpload,
    models::{
        canonical::CanonicalModel,
        draft::{DraftDocument, ManualDocument},
    },
    pipeline::garage::GaragePipelineService,
    postgres::{PostgresReviewStore, PublishedDocument, PublishedDocumentSummary},
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
    uploads: DocumentUpload,
    restate: RestateClient,
    pdfs: GaragePipelineService,
    drafts: PostgresReviewStore,
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
}

#[derive(Deserialize, ToSchema)]
struct RepairDocumentRequest {
    manual_data: ManualDocument,
    #[serde(default)]
    enrich: bool,
}

struct ApiError(StatusCode, String);

#[derive(OpenApi)]
#[openapi(
    info(
        title = "SCEPA API",
        description = "Document ingestion, review, and publication API"
    ),
    paths(
        upload_pdf,
        download_pdf,
        submit_pdf,
        get_draft,
        publish_draft,
        list_documents,
        list_documents_requiring_fixing,
        get_document_requiring_fixing,
        fix_document,
        get_published_document,
        update_document
    ),
    tags(
        (name = "documents", description = "Document ingestion and publication"),
        (name = "review", description = "Operator review and repair")
    )
)]
struct ApiDoc;

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, self.1).into_response()
    }
}

/// Builds the document-upload HTTP router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/openapi.json", openapi()))
        .route("/pdfs", post(upload_pdf))
        .route("/pdfs/{pdf_hash}", get(download_pdf))
        .route("/pdfs/submissions/{workflow_id}", post(submit_pdf))
        .route("/drafts/{pdf_hash}", get(get_draft).put(publish_draft))
        .route("/documents", get(list_documents))
        .route(
            "/documents/requiring-fixing",
            get(list_documents_requiring_fixing),
        )
        .route(
            "/documents/requiring-fixing/{case_id}",
            get(get_document_requiring_fixing).put(fix_document),
        )
        .route(
            "/documents/{pdf_hash}",
            get(get_published_document).put(update_document),
        )
        .layer(DefaultBodyLimit::max(MAX_PDF_BYTES))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Generates the OpenAPI 3.1 document for the public HTTP API.
pub fn openapi() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

#[utoipa::path(
    get,
    path = "/pdfs/{pdf_hash}",
    params(("pdf_hash" = String, Path, description = "SHA-256 hash of the PDF")),
    responses(
        (status = 200, description = "PDF bytes", body = Vec<u8>, content_type = "application/pdf"),
        (status = 404, description = "PDF not found", body = String, content_type = "text/plain"),
        (status = 500, description = "Storage error", body = String, content_type = "text/plain")
    ),
    tag = "documents"
)]
async fn download_pdf(
    State(state): State<AppState>,
    Path(pdf_hash): Path<String>,
) -> Result<Response, ApiError> {
    let (metadata, bytes) = state
        .pdfs
        .load(&pdf_hash)
        .await
        .map_err(internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "PDF not found".into()))?;
    let content_type = HeaderValue::from_str(&metadata.content_type).map_err(internal)?;
    let content_disposition =
        HeaderValue::from_str(&format!("inline; filename=\"{}.pdf\"", metadata.pdf_hash))
            .map_err(internal)?;
    let etag = HeaderValue::from_str(&format!("\"{}\"", metadata.pdf_hash)).map_err(internal)?;

    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, content_disposition),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=31536000, immutable"),
            ),
            (header::ETAG, etag),
        ],
        bytes,
    )
        .into_response())
}

#[utoipa::path(
    post,
    path = "/pdfs",
    request_body(content = Vec<u8>, content_type = "application/pdf", description = "PDF document"),
    responses(
        (status = 201, description = "Document processed", body = UploadResponse),
        (status = 502, description = "Pipeline or workflow error", body = String, content_type = "text/plain")
    ),
    tag = "documents"
)]
async fn upload_pdf(
    State(state): State<AppState>,
    pdf: Bytes,
) -> Result<(StatusCode, Json<UploadResponse>), ApiError> {
    let upload = state.uploads.run(pdf.to_vec()).await.map_err(upstream)?;

    Ok((
        StatusCode::CREATED,
        Json(UploadResponse {
            workflow_id: upload.workflow_id,
            result: upload.result,
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/pdfs/submissions/{workflow_id}",
    params(("workflow_id" = String, Path, description = "Client-selected workflow identifier")),
    request_body(content = Vec<u8>, content_type = "application/pdf", description = "PDF document"),
    responses(
        (status = 202, description = "Submission accepted", body = SubmissionResponse),
        (status = 400, description = "Invalid submission", body = String, content_type = "text/plain"),
        (status = 502, description = "Pipeline or workflow error", body = String, content_type = "text/plain")
    ),
    tag = "documents"
)]
async fn submit_pdf(
    State(state): State<AppState>,
    Path(workflow_id): Path<String>,
    pdf: Bytes,
) -> Result<(StatusCode, Json<SubmissionResponse>), ApiError> {
    state
        .uploads
        .submit(&workflow_id, pdf.to_vec())
        .await
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::InvalidInput {
                ApiError(StatusCode::BAD_REQUEST, error.to_string())
            } else {
                upstream(error)
            }
        })?;

    Ok((
        StatusCode::ACCEPTED,
        Json(SubmissionResponse { workflow_id }),
    ))
}

#[utoipa::path(
    get,
    path = "/drafts/{pdf_hash}",
    params(("pdf_hash" = String, Path, description = "SHA-256 hash of the PDF")),
    responses(
        (status = 200, description = "Document draft", body = DraftResponse),
        (status = 404, description = "Draft not found", body = String, content_type = "text/plain"),
        (status = 500, description = "Persistence error", body = String, content_type = "text/plain")
    ),
    tag = "documents"
)]
async fn get_draft(
    State(state): State<AppState>,
    Path(pdf_hash): Path<String>,
) -> Result<Json<DraftResponse>, ApiError> {
    let draft = state
        .drafts
        .get_draft_document(&pdf_hash)
        .await
        .map_err(internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "document draft not found".into()))?;

    Ok(Json(DraftResponse { pdf_hash, draft }))
}

#[utoipa::path(
    put,
    path = "/drafts/{pdf_hash}",
    params(("pdf_hash" = String, Path, description = "SHA-256 hash of the PDF")),
    request_body(content = ManualDocument, description = "Manual document corrections"),
    responses(
        (status = 200, description = "Published document", body = PublishResponse),
        (status = 502, description = "Workflow error", body = String, content_type = "text/plain")
    ),
    tag = "documents"
)]
async fn publish_draft(
    State(state): State<AppState>,
    Path(pdf_hash): Path<String>,
    Json(manual_data): Json<ManualDocument>,
) -> Result<Json<PublishResponse>, ApiError> {
    let published = state
        .restate
        .publish_draft(pdf_hash.clone(), manual_data)
        .await
        .map_err(upstream)?;

    Ok(Json(PublishResponse {
        artifact: DraftResponse {
            pdf_hash,
            draft: published.artifact,
        },
        canonical: published.canonical,
    }))
}

#[utoipa::path(
    get,
    path = "/documents",
    responses(
        (status = 200, description = "Published document summaries", body = Vec<PublishedDocumentSummary>),
        (status = 500, description = "Persistence error", body = String, content_type = "text/plain")
    ),
    tag = "documents"
)]
async fn list_documents(
    State(state): State<AppState>,
) -> Result<Json<Vec<PublishedDocumentSummary>>, ApiError> {
    Ok(Json(
        state
            .drafts
            .list_published_documents()
            .await
            .map_err(internal)?,
    ))
}

#[utoipa::path(
    get,
    path = "/documents/requiring-fixing",
    responses(
        (status = 200, description = "Pending review cases", body = Vec<scepa::postgres::ReviewCase>),
        (status = 500, description = "Persistence error", body = String, content_type = "text/plain")
    ),
    tag = "review"
)]
async fn list_documents_requiring_fixing(
    State(state): State<AppState>,
) -> Result<Json<Vec<scepa::postgres::ReviewCase>>, ApiError> {
    Ok(Json(
        state
            .drafts
            .list_documents_requiring_fixing()
            .await
            .map_err(internal)?,
    ))
}

#[utoipa::path(
    get,
    path = "/documents/requiring-fixing/{case_id}",
    params(("case_id" = i64, Path, description = "Review case identifier")),
    responses(
        (status = 200, description = "Review case and repair draft", body = RepairDraftResponse),
        (status = 404, description = "Pending review case not found", body = String, content_type = "text/plain"),
        (status = 409, description = "Review case has no source PDF", body = String, content_type = "text/plain"),
        (status = 500, description = "Persistence error", body = String, content_type = "text/plain")
    ),
    tag = "review"
)]
async fn get_document_requiring_fixing(
    State(state): State<AppState>,
    Path(case_id): Path<i64>,
) -> Result<Json<RepairDraftResponse>, ApiError> {
    let repair = load_repair(&state.drafts, case_id).await?;

    Ok(Json(RepairDraftResponse {
        case: repair.case,
        draft: DraftResponse {
            pdf_hash: repair.pdf_hash,
            draft: repair.draft,
        },
    }))
}

#[utoipa::path(
    put,
    path = "/documents/requiring-fixing/{case_id}",
    params(("case_id" = i64, Path, description = "Review case identifier")),
    request_body(content = RepairDocumentRequest),
    responses(
        (status = 200, description = "Repaired and published document", body = PublishResponse),
        (status = 502, description = "Workflow error", body = String, content_type = "text/plain")
    ),
    tag = "review"
)]
async fn fix_document(
    State(state): State<AppState>,
    Path(case_id): Path<i64>,
    Json(request): Json<RepairDocumentRequest>,
) -> Result<Json<PublishResponse>, ApiError> {
    let result = state
        .restate
        .fix_document(FixDocumentWorkflowRequest {
            case_id,
            manual_data: request.manual_data,
            enrich: request.enrich,
        })
        .await
        .map_err(upstream)?;

    Ok(Json(PublishResponse {
        artifact: DraftResponse {
            pdf_hash: result.pdf_hash,
            draft: result.artifact,
        },
        canonical: result.canonical,
    }))
}

#[utoipa::path(
    get,
    path = "/documents/{pdf_hash}",
    params(("pdf_hash" = String, Path, description = "SHA-256 hash of the PDF")),
    responses(
        (status = 200, description = "Published document", body = PublishedDocument),
        (status = 404, description = "Published document not found", body = String, content_type = "text/plain"),
        (status = 500, description = "Persistence error", body = String, content_type = "text/plain")
    ),
    tag = "documents"
)]
async fn get_published_document(
    State(state): State<AppState>,
    Path(pdf_hash): Path<String>,
) -> Result<Json<PublishedDocument>, ApiError> {
    state
        .drafts
        .get_published_document(&pdf_hash)
        .await
        .map_err(internal)?
        .map(Json)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "published document not found".into()))
}

#[utoipa::path(
    put,
    path = "/documents/{pdf_hash}",
    params(("pdf_hash" = String, Path, description = "SHA-256 hash of the PDF")),
    request_body(content = ManualDocument, description = "Manual document corrections"),
    responses(
        (status = 200, description = "Updated document", body = UpdateDocumentWorkflowResponse),
        (status = 502, description = "Workflow error", body = String, content_type = "text/plain")
    ),
    tag = "documents"
)]
async fn update_document(
    State(state): State<AppState>,
    Path(pdf_hash): Path<String>,
    Json(manual_data): Json<ManualDocument>,
) -> Result<Json<UpdateDocumentWorkflowResponse>, ApiError> {
    let result = state
        .restate
        .update_document(pdf_hash, manual_data)
        .await
        .map_err(upstream)?;
    Ok(Json(result))
}

async fn load_repair(drafts: &PostgresReviewStore, case_id: i64) -> Result<RepairDraft, ApiError> {
    let case = drafts
        .get_case(case_id)
        .await
        .map_err(internal)?
        .filter(|case| case.status == "pending")
        .ok_or_else(|| {
            ApiError(
                StatusCode::NOT_FOUND,
                "pending review case not found".into(),
            )
        })?;
    let pdf_hash = case.pdf_hash.clone().ok_or_else(|| {
        ApiError(
            StatusCode::CONFLICT,
            "review case is not linked to a source PDF".into(),
        )
    })?;
    let draft = drafts.get_repair_draft(&pdf_hash).await.map_err(internal)?;
    Ok(RepairDraft {
        case,
        pdf_hash,
        draft,
    })
}

fn internal(error: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn upstream(error: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_GATEWAY, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            ("DraftDocument", "grobid_extraction_data"),
            ("PublishedDocumentSummary", "identifiers"),
            ("ReviewCase", "workflow_id"),
            ("CanonicalModel", "publication_events"),
            ("NewDocumentWorkflowResponse", "stored_pdf"),
            ("UpdateDocumentWorkflowResponse", "changes"),
            ("ResearchPaper", "doi"),
            ("CanonicalUpdateSummary", "contributors_inserted"),
        ] {
            assert!(
                schemas[name]["properties"].get(property).is_some(),
                "{name} is missing its {property} property: {}",
                schemas[name]
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
    }
}
