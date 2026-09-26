//! Public Axum API for uploading new documents.

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use scepa::{
    document_upload::{DocumentUpload, DocumentUploadError},
    models::{
        canonical::{CanonicalMissingField, CanonicalModel, canonical_missing_fields},
        draft::{DraftDocument, ManualDocument},
    },
    pipeline::garage::GaragePipelineService,
    postgres::{
        PostgresReviewStore, PublishedDocument, PublishedDocumentSummary, ReviewCaseDocumentSummary,
    },
    restate::{
        RestateClient, RestateError, RestateErrorKind,
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
    missing_fields: Vec<CanonicalMissingField>,
}

#[derive(Deserialize, ToSchema)]
struct RepairDocumentRequest {
    manual_data: ManualDocument,
    #[serde(default)]
    enrich: bool,
}

#[derive(Debug, Serialize, ToSchema)]
struct ErrorResponse {
    error: String,
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    workflow_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pdf_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    review_case_id: Option<i64>,
}

#[derive(Serialize, ToSchema)]
struct DocumentValidationErrorResponse {
    error: String,
    action: String,
    code: String,
    workflow_id: String,
    pdf_hash: String,
    review_case_id: i64,
    missing_fields: Vec<CanonicalMissingField>,
}

#[allow(dead_code)]
#[derive(ToSchema)]
#[serde(untagged)]
enum UploadErrorResponse {
    General(ErrorResponse),
    Validation(DocumentValidationErrorResponse),
}

#[derive(Debug)]
struct ApiError(StatusCode, ErrorResponse);

impl ApiError {
    fn with_pdf_hash(mut self, pdf_hash: String) -> Self {
        self.1.pdf_hash = Some(pdf_hash);
        self
    }

    fn with_review_case_id(mut self, review_case_id: i64) -> Self {
        self.1.review_case_id = Some(review_case_id);
        self
    }
}

#[derive(Clone, Copy)]
struct StructuredApiError;

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
        let mut response = (self.0, Json(self.1)).into_response();
        response.extensions_mut().insert(StructuredApiError);
        response
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
        .layer(middleware::from_fn(normalize_error_response))
        .with_state(state)
}

async fn normalize_error_response(request: Request<Body>, next: Next) -> Response {
    normalize_framework_error(next.run(request).await)
}

fn normalize_framework_error(mut response: Response) -> Response {
    let status = response.status();
    if !(status.is_client_error() || status.is_server_error())
        || response.extensions().get::<StructuredApiError>().is_some()
    {
        return response;
    }

    let (message, action) = match status {
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => {
            ("Invalid request", "Correct the request data and try again")
        }
        StatusCode::NOT_FOUND => ("Resource not found", "Refresh the page and try again"),
        StatusCode::METHOD_NOT_ALLOWED => (
            "Method not allowed",
            "Use one of the methods allowed by this endpoint",
        ),
        StatusCode::PAYLOAD_TOO_LARGE => (
            "Request body too large",
            "Submit a file smaller than 64 MiB",
        ),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => (
            "Unsupported media type",
            "Submit the request using the documented content type",
        ),
        StatusCode::BAD_GATEWAY => (
            "An upstream service returned an invalid response",
            "Try again; if the problem continues, contact support",
        ),
        StatusCode::SERVICE_UNAVAILABLE => (
            "A required service is temporarily unavailable",
            "Wait a moment and try again",
        ),
        StatusCode::GATEWAY_TIMEOUT => (
            "A required service timed out",
            "Try again; if the problem continues, contact support",
        ),
        status if status.is_client_error() => {
            ("Request failed", "Correct the request and try again")
        }
        _ => (
            "Internal server error",
            "Try again; if the problem continues, contact support",
        ),
    };
    let json_response = Json(ErrorResponse {
        error: message.into(),
        action: action.into(),
        workflow_id: None,
        pdf_hash: None,
        review_case_id: None,
    })
    .into_response();

    *response.body_mut() = json_response.into_body();
    response.headers_mut().remove(header::CONTENT_LENGTH);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
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
        (status = 404, description = "PDF not found", body = ErrorResponse),
        (status = 500, description = "Storage error", body = ErrorResponse)
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
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "PDF not found",
                "Upload the PDF again or verify its hash",
            )
            .with_pdf_hash(pdf_hash.clone())
        })?;
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
        (status = 413, description = "PDF exceeds the upload limit", body = ErrorResponse),
        (status = 422, description = "PDF or extracted document requires correction", body = UploadErrorResponse),
        (status = 409, description = "Conflicting data or workflow identity", body = ErrorResponse),
        (status = 404, description = "Stored workflow artifact not found", body = ErrorResponse),
        (status = 500, description = "Document processing failed", body = ErrorResponse),
        (status = 502, description = "Invalid upstream response", body = ErrorResponse),
        (status = 503, description = "Document processing unavailable", body = ErrorResponse),
        (status = 504, description = "Document processing timed out", body = ErrorResponse)
    ),
    tag = "documents"
)]
async fn upload_pdf(State(state): State<AppState>, pdf: Bytes) -> Result<Response, ApiError> {
    let upload = match state.uploads.run(pdf.to_vec()).await {
        Ok(upload) => upload,
        Err(failure) => {
            if let Some(validation) =
                upload_validation_response(&state, &failure.workflow_id).await?
            {
                let mut response =
                    (StatusCode::UNPROCESSABLE_ENTITY, Json(validation)).into_response();
                response.extensions_mut().insert(StructuredApiError);
                return Ok(response);
            }
            return Err(
                document_upload_error(&state, &failure.workflow_id, failure.source, None).await,
            );
        }
    };

    Ok((
        StatusCode::CREATED,
        Json(UploadResponse {
            workflow_id: upload.workflow_id,
            result: upload.result,
        }),
    )
        .into_response())
}

async fn upload_validation_response(
    state: &AppState,
    workflow_id: &str,
) -> Result<Option<DocumentValidationErrorResponse>, ApiError> {
    let Some(case) = state
        .drafts
        .get_pending_case_for_workflow(workflow_id, "typedb", "input_validation")
        .await
        .map_err(internal)?
        .filter(|case| !case.retryable)
    else {
        return Ok(None);
    };
    let Some(pdf_hash) = case.pdf_hash.clone() else {
        return Ok(None);
    };
    let draft = state
        .drafts
        .get_repair_draft(&pdf_hash)
        .await
        .map_err(internal)?;
    Ok(document_validation_response(
        workflow_id,
        case.id,
        pdf_hash,
        &draft,
    ))
}

fn document_validation_response(
    workflow_id: &str,
    review_case_id: i64,
    pdf_hash: String,
    draft: &DraftDocument,
) -> Option<DocumentValidationErrorResponse> {
    let missing_fields = canonical_missing_fields(&draft.effective_document());
    if missing_fields.is_empty() {
        return None;
    }

    Some(DocumentValidationErrorResponse {
        error: "Document requires correction".into(),
        action: "Open the review case and supply the missing document fields".into(),
        code: "document_validation_failed".into(),
        workflow_id: workflow_id.into(),
        pdf_hash,
        review_case_id,
        missing_fields,
    })
}

#[utoipa::path(
    post,
    path = "/pdfs/submissions/{workflow_id}",
    params(("workflow_id" = String, Path, description = "Client-selected workflow identifier")),
    request_body(content = Vec<u8>, content_type = "application/pdf", description = "PDF document"),
    responses(
        (status = 202, description = "Submission accepted", body = SubmissionResponse),
        (status = 400, description = "Invalid submission", body = ErrorResponse),
        (status = 413, description = "PDF exceeds the upload limit", body = ErrorResponse),
        (status = 422, description = "Invalid PDF", body = ErrorResponse),
        (status = 409, description = "Conflicting data or workflow identity", body = ErrorResponse),
        (status = 500, description = "Document storage failed", body = ErrorResponse),
        (status = 502, description = "Invalid upstream response", body = ErrorResponse),
        (status = 503, description = "Document processing unavailable", body = ErrorResponse),
        (status = 504, description = "Document processing timed out", body = ErrorResponse)
    ),
    tag = "documents"
)]
async fn submit_pdf(
    State(state): State<AppState>,
    Path(workflow_id): Path<String>,
    pdf: Bytes,
) -> Result<(StatusCode, Json<SubmissionResponse>), ApiError> {
    if let Err(error) = state.uploads.submit(&workflow_id, pdf.to_vec()).await {
        return Err(document_upload_error(&state, &workflow_id, error, None).await);
    }

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
        (status = 404, description = "Draft not found", body = ErrorResponse),
        (status = 500, description = "Persistence error", body = ErrorResponse)
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
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "Document draft not found",
                "Upload the document again or refresh the document list",
            )
            .with_pdf_hash(pdf_hash.clone())
        })?;

    Ok(Json(DraftResponse { pdf_hash, draft }))
}

#[utoipa::path(
    put,
    path = "/drafts/{pdf_hash}",
    params(
        ("pdf_hash" = String, Path, description = "SHA-256 hash of the PDF"),
        ("Idempotency-Key" = Option<String>, Header, description = "Required when the request creates objects without IDs")
    ),
    request_body(content = ManualDocument, description = "Manual document corrections"),
    responses(
        (status = 200, description = "Published document", body = PublishResponse),
        (status = 400, description = "Invalid request body", body = ErrorResponse),
        (status = 413, description = "Request body exceeds the upload limit", body = ErrorResponse),
        (status = 415, description = "Unsupported media type", body = ErrorResponse),
        (status = 422, description = "Invalid request data", body = ErrorResponse),
        (status = 409, description = "Conflicting data or workflow identity", body = ErrorResponse),
        (status = 404, description = "Workflow artifact not found", body = ErrorResponse),
        (status = 500, description = "Document processing failed", body = ErrorResponse),
        (status = 502, description = "Invalid upstream response", body = ErrorResponse),
        (status = 503, description = "Document processing unavailable", body = ErrorResponse),
        (status = 504, description = "Document processing timed out", body = ErrorResponse)
    ),
    tag = "documents"
)]
async fn publish_draft(
    State(state): State<AppState>,
    Path(pdf_hash): Path<String>,
    headers: HeaderMap,
    Json(mut manual_data): Json<ManualDocument>,
) -> Result<Json<PublishResponse>, ApiError> {
    assign_request_ids(&pdf_hash, &headers, &mut manual_data)?;
    validate_manual_update(&state, &pdf_hash, &manual_data).await?;
    let published = match state
        .restate
        .publish_draft(pdf_hash.clone(), manual_data)
        .await
    {
        Ok(published) => published,
        Err(error) => {
            return Err(restate_api_error(&state, error, Some(pdf_hash), None).await);
        }
    };

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
        (status = 500, description = "Persistence error", body = ErrorResponse)
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
        (status = 200, description = "Pending review cases with document summaries", body = Vec<ReviewCaseDocumentSummary>),
        (status = 500, description = "Persistence error", body = ErrorResponse)
    ),
    tag = "review"
)]
async fn list_documents_requiring_fixing(
    State(state): State<AppState>,
) -> Result<Json<Vec<ReviewCaseDocumentSummary>>, ApiError> {
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
        (status = 400, description = "Invalid review case identifier", body = ErrorResponse),
        (status = 404, description = "Pending review case not found", body = ErrorResponse),
        (status = 409, description = "Review case has no source PDF", body = ErrorResponse),
        (status = 500, description = "Persistence error", body = ErrorResponse)
    ),
    tag = "review"
)]
async fn get_document_requiring_fixing(
    State(state): State<AppState>,
    Path(case_id): Path<i64>,
) -> Result<Json<RepairDraftResponse>, ApiError> {
    let repair = load_repair(&state.drafts, case_id).await?;
    let missing_fields = canonical_missing_fields(&repair.draft.effective_document());

    Ok(Json(RepairDraftResponse {
        case: repair.case,
        draft: DraftResponse {
            pdf_hash: repair.pdf_hash,
            draft: repair.draft,
        },
        missing_fields,
    }))
}

#[utoipa::path(
    put,
    path = "/documents/requiring-fixing/{case_id}",
    params(
        ("case_id" = i64, Path, description = "Review case identifier"),
        ("Idempotency-Key" = Option<String>, Header, description = "Required when the request creates objects without IDs")
    ),
    request_body(content = RepairDocumentRequest),
    responses(
        (status = 200, description = "Repaired and published document", body = PublishResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 413, description = "Request body exceeds the upload limit", body = ErrorResponse),
        (status = 415, description = "Unsupported media type", body = ErrorResponse),
        (status = 422, description = "Invalid request data", body = ErrorResponse),
        (status = 409, description = "Conflicting data or workflow identity", body = ErrorResponse),
        (status = 404, description = "Workflow artifact not found", body = ErrorResponse),
        (status = 500, description = "Document processing failed", body = ErrorResponse),
        (status = 502, description = "Invalid upstream response", body = ErrorResponse),
        (status = 503, description = "Document processing unavailable", body = ErrorResponse),
        (status = 504, description = "Document processing timed out", body = ErrorResponse)
    ),
    tag = "review"
)]
async fn fix_document(
    State(state): State<AppState>,
    Path(case_id): Path<i64>,
    headers: HeaderMap,
    Json(mut request): Json<RepairDocumentRequest>,
) -> Result<Json<PublishResponse>, ApiError> {
    let repair = load_repair(&state.drafts, case_id).await?;
    assign_request_ids(&repair.pdf_hash, &headers, &mut request.manual_data)?;
    validate_artifact_update(repair.draft, &request.manual_data)?;
    let result = match state
        .restate
        .fix_document(FixDocumentWorkflowRequest {
            case_id,
            manual_data: request.manual_data,
            enrich: request.enrich,
        })
        .await
    {
        Ok(result) => result,
        Err(error) => {
            return Err(restate_api_error(
                &state,
                error,
                Some(repair.pdf_hash.clone()),
                Some(case_id),
            )
            .await);
        }
    };

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
        (status = 404, description = "Published document not found", body = ErrorResponse),
        (status = 500, description = "Persistence error", body = ErrorResponse)
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
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "Published document not found",
                "Refresh the document list or publish the document first",
            )
            .with_pdf_hash(pdf_hash.clone())
        })
}

#[utoipa::path(
    put,
    path = "/documents/{pdf_hash}",
    params(
        ("pdf_hash" = String, Path, description = "SHA-256 hash of the PDF"),
        ("Idempotency-Key" = Option<String>, Header, description = "Required when the request creates objects without IDs")
    ),
    request_body(content = ManualDocument, description = "Manual document corrections"),
    responses(
        (status = 200, description = "Updated document", body = UpdateDocumentWorkflowResponse),
        (status = 400, description = "Invalid request body", body = ErrorResponse),
        (status = 413, description = "Request body exceeds the upload limit", body = ErrorResponse),
        (status = 415, description = "Unsupported media type", body = ErrorResponse),
        (status = 422, description = "Invalid request data", body = ErrorResponse),
        (status = 409, description = "Conflicting data or workflow identity", body = ErrorResponse),
        (status = 404, description = "Workflow artifact not found", body = ErrorResponse),
        (status = 500, description = "Document processing failed", body = ErrorResponse),
        (status = 502, description = "Invalid upstream response", body = ErrorResponse),
        (status = 503, description = "Document processing unavailable", body = ErrorResponse),
        (status = 504, description = "Document processing timed out", body = ErrorResponse)
    ),
    tag = "documents"
)]
async fn update_document(
    State(state): State<AppState>,
    Path(pdf_hash): Path<String>,
    headers: HeaderMap,
    Json(mut manual_data): Json<ManualDocument>,
) -> Result<Json<UpdateDocumentWorkflowResponse>, ApiError> {
    assign_request_ids(&pdf_hash, &headers, &mut manual_data)?;
    validate_manual_update(&state, &pdf_hash, &manual_data).await?;
    let result = match state
        .restate
        .update_document(pdf_hash.clone(), manual_data)
        .await
    {
        Ok(result) => result,
        Err(error) => {
            return Err(restate_api_error(&state, error, Some(pdf_hash), None).await);
        }
    };
    Ok(Json(result))
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

fn internal(error: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %error, "API request failed internally");
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Internal server error",
        "Try again; if the problem continues, contact support",
    )
}

fn api_error(status: StatusCode, error: impl Into<String>, action: impl Into<String>) -> ApiError {
    ApiError(
        status,
        ErrorResponse {
            error: error.into(),
            action: action.into(),
            workflow_id: None,
            pdf_hash: None,
            review_case_id: None,
        },
    )
}

async fn document_upload_error(
    state: &AppState,
    workflow_id: &str,
    error: DocumentUploadError,
    pdf_hash: Option<String>,
) -> ApiError {
    match error {
        DocumentUploadError::InvalidInput(message) => {
            let mut error = api_error(
                StatusCode::BAD_REQUEST,
                message,
                "Provide a non-empty workflow identifier and try again",
            );
            error.1.workflow_id = (!workflow_id.is_empty()).then(|| workflow_id.to_owned());
            error
        }
        DocumentUploadError::Storage(source) => {
            if let Some(conflict) = source
                .get_ref()
                .and_then(|error| error.downcast_ref::<scepa::conflict::Conflict>())
            {
                let mut error = api_error(
                    StatusCode::CONFLICT,
                    conflict.to_string(),
                    "Use a new workflow identifier or refresh the existing submission",
                );
                error.1.workflow_id = Some(workflow_id.to_owned());
                return error;
            }
            tracing::error!(error = %source, %workflow_id, "PDF storage pipeline failed");
            let case = latest_review_case(state, workflow_id).await;
            let mut error = if case
                .as_ref()
                .is_some_and(|case| case.service == "garage" && case.phase == "input_validation")
            {
                api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "The uploaded file is not a valid PDF",
                    "Choose a non-empty PDF file and upload it again",
                )
            } else if case.is_some() {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "The PDF could not be stored correctly",
                    "Try again; if the problem continues, contact support",
                )
            } else {
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Document storage is temporarily unavailable",
                    "Wait a moment and upload the PDF again",
                )
            };
            add_error_context(&mut error, workflow_id, pdf_hash, case.as_ref());
            error
        }
        DocumentUploadError::Workflow(error) => {
            restate_api_error(state, error, pdf_hash, None).await
        }
    }
}

async fn restate_api_error(
    state: &AppState,
    error: RestateError,
    pdf_hash: Option<String>,
    review_case_id: Option<i64>,
) -> ApiError {
    tracing::error!(error = %error, "API request failed through Restate");
    let workflow_id = error.workflow_id().to_owned();
    let mut api_error = classify_restate_error(error.kind(), error.rejection_message());

    let case = latest_review_case(state, &workflow_id).await;
    add_error_context(&mut api_error, &workflow_id, pdf_hash, case.as_ref());
    if api_error.1.review_case_id.is_none() {
        api_error.1.review_case_id = review_case_id;
    }
    api_error
}

fn classify_restate_error(kind: RestateErrorKind, rejection_message: Option<&str>) -> ApiError {
    match kind {
        RestateErrorKind::InvalidRequest => api_error(
            StatusCode::BAD_REQUEST,
            "The workflow request is invalid",
            "Correct the request identifiers and try again",
        ),
        RestateErrorKind::Unavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Document processing is temporarily unavailable",
            "Wait a moment and try again",
        ),
        RestateErrorKind::Timeout => api_error(
            StatusCode::GATEWAY_TIMEOUT,
            "Document processing timed out",
            "Try again; if the problem continues, contact support",
        ),
        RestateErrorKind::InvalidResponse => api_error(
            StatusCode::BAD_GATEWAY,
            "Document processing returned an invalid response",
            "Try again; if the problem continues, contact support",
        ),
        RestateErrorKind::Rejected(404) => api_error(
            StatusCode::NOT_FOUND,
            "A required document artifact was not found",
            "Upload the document again or refresh the document list",
        ),
        RestateErrorKind::Rejected(409) => {
            let message = rejection_message
                .and_then(scepa::conflict::Conflict::from_message)
                .map(|conflict| conflict.to_string())
                .unwrap_or_else(|| "The workflow conflicts with existing data".into());
            api_error(
                StatusCode::CONFLICT,
                message,
                "Refresh the current data or use a new workflow identifier",
            )
        }
        RestateErrorKind::Rejected(422) => {
            let enrichment = rejection_message == Some("External enrichment is not available");
            api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                if enrichment {
                    "External enrichment is not available"
                } else {
                    "The document could not be processed"
                },
                if enrichment {
                    "Submit the repair again with enrichment disabled"
                } else {
                    "Review the document and correct its contents before trying again"
                },
            )
        }
        RestateErrorKind::Rejected(429 | 503) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Document processing is temporarily unavailable",
            "Wait a moment and try again",
        ),
        RestateErrorKind::Rejected(408 | 504) => api_error(
            StatusCode::GATEWAY_TIMEOUT,
            "Document processing timed out",
            "Try again; if the problem continues, contact support",
        ),
        RestateErrorKind::Rejected(500) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Document processing failed",
            "Try again; if the problem continues, contact support",
        ),
        RestateErrorKind::Rejected(status) if (400..500).contains(&status) => api_error(
            StatusCode::BAD_REQUEST,
            "The workflow request was rejected",
            "Correct the request and try again",
        ),
        RestateErrorKind::Rejected(_) => api_error(
            StatusCode::BAD_GATEWAY,
            "Document processing returned an unexpected error",
            "Try again; if the problem continues, contact support",
        ),
    }
}

async fn latest_review_case(
    state: &AppState,
    workflow_id: &str,
) -> Option<scepa::postgres::ReviewCase> {
    match state
        .drafts
        .get_latest_pending_case_for_workflow(workflow_id)
        .await
    {
        Ok(case) => case,
        Err(error) => {
            tracing::error!(error = %error, %workflow_id, "could not load workflow review context");
            None
        }
    }
}

fn add_error_context(
    error: &mut ApiError,
    workflow_id: &str,
    pdf_hash: Option<String>,
    case: Option<&scepa::postgres::ReviewCase>,
) {
    error.1.workflow_id = Some(workflow_id.to_owned());
    error.1.pdf_hash = case.and_then(|case| case.pdf_hash.clone()).or(pdf_hash);
    error.1.review_case_id = case.map(|case| case.id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::to_bytes,
        http::{Method, Request},
    };
    use scepa::models::draft::{Bibliography, PassageLevel, TeiDocument};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    async fn assert_error_response(response: Response, status: StatusCode, message: &str) {
        assert_eq!(response.status(), status);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = serde_json::from_slice::<Value>(&body).unwrap();
        assert_eq!(body["error"], message);
        assert!(
            body["action"]
                .as_str()
                .is_some_and(|action| !action.is_empty())
        );
    }

    fn rejection_test_router() -> Router {
        async fn accept_json(Path(_id): Path<u64>, Json(_body): Json<Value>) -> StatusCode {
            StatusCode::NO_CONTENT
        }

        Router::new()
            .route("/items/{id}", post(accept_json))
            .layer(DefaultBodyLimit::max(16))
            .layer(middleware::from_fn(normalize_error_response))
    }

    async fn rejection_response(method: Method, uri: &str, body: &str) -> Response {
        rejection_test_router()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_owned()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn conflicts_reach_http_as_safe_json() {
        for conflict in [
            scepa::conflict::Conflict::WorkflowPdf,
            scepa::conflict::Conflict::Record,
            scepa::conflict::Conflict::CanonicalIdentity,
            scepa::conflict::Conflict::PassageIdentity,
            scepa::conflict::Conflict::Submission,
        ] {
            assert_error_response(
                normalize_framework_error(
                    api_error(
                        StatusCode::CONFLICT,
                        conflict.to_string(),
                        "Refresh the current data or use a new workflow identifier",
                    )
                    .into_response(),
                ),
                StatusCode::CONFLICT,
                &conflict.to_string(),
            )
            .await;
        }
    }

    #[tokio::test]
    async fn upload_validation_response_survives_http_error_normalization() {
        let draft = DraftDocument::new(TeiDocument {
            id: String::new(),
            level: PassageLevel::Paragraph,
            bibliography: Bibliography::default(),
            body_text: Vec::new(),
            figures_and_tables: Vec::new(),
        });

        let response =
            document_validation_response("upload:workflow-1", 42, "a".repeat(64), &draft).unwrap();
        assert_eq!(response.code, "document_validation_failed");
        assert!(!response.action.is_empty());
        assert_eq!(response.workflow_id, "upload:workflow-1");
        assert_eq!(response.review_case_id, 42);
        assert_eq!(
            response
                .missing_fields
                .iter()
                .map(|field| field.path.as_str())
                .collect::<Vec<_>>(),
            vec!["bibliography.title", "bibliography.authors"]
        );

        let mut http_response = (StatusCode::UNPROCESSABLE_ENTITY, Json(response)).into_response();
        http_response.extensions_mut().insert(StructuredApiError);
        let normalized = normalize_framework_error(http_response);
        assert_eq!(normalized.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = to_bytes(normalized.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["code"], "document_validation_failed");
        assert_eq!(body["review_case_id"], 42);
        assert_eq!(body["missing_fields"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn api_errors_are_json_objects_with_safe_messages() {
        assert_error_response(
            api_error(
                StatusCode::NOT_FOUND,
                "PDF not found",
                "Upload the PDF again",
            )
            .into_response(),
            StatusCode::NOT_FOUND,
            "PDF not found",
        )
        .await;
        assert_error_response(
            internal("database password leaked").into_response(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal server error",
        )
        .await;
    }

    #[test]
    fn restate_failures_use_semantic_statuses_without_leaking_rejection_details() {
        for (kind, expected) in [
            (RestateErrorKind::InvalidRequest, StatusCode::BAD_REQUEST),
            (RestateErrorKind::Rejected(404), StatusCode::NOT_FOUND),
            (RestateErrorKind::Rejected(409), StatusCode::CONFLICT),
            (
                RestateErrorKind::Rejected(422),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                RestateErrorKind::Rejected(500),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                RestateErrorKind::Rejected(429),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (RestateErrorKind::Rejected(408), StatusCode::GATEWAY_TIMEOUT),
            (RestateErrorKind::InvalidResponse, StatusCode::BAD_GATEWAY),
            (
                RestateErrorKind::Unavailable,
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (RestateErrorKind::Timeout, StatusCode::GATEWAY_TIMEOUT),
        ] {
            let error = classify_restate_error(kind, Some("database password leaked"));
            assert_eq!(error.0, expected);
            let body = serde_json::to_string(&error.1).unwrap();
            assert!(!body.contains("database password leaked"));
            assert!(!error.1.action.is_empty());
        }
    }

    #[tokio::test]
    async fn framework_rejections_are_normalized_as_json() {
        for (method, uri, body, status, message) in [
            (
                Method::POST,
                "/items/1",
                "{",
                StatusCode::BAD_REQUEST,
                "Invalid request",
            ),
            (
                Method::POST,
                "/items/not-a-number",
                "{}",
                StatusCode::BAD_REQUEST,
                "Invalid request",
            ),
            (
                Method::POST,
                "/items/1",
                r#"{"value":"too large"}"#,
                StatusCode::PAYLOAD_TOO_LARGE,
                "Request body too large",
            ),
            (
                Method::GET,
                "/missing",
                "",
                StatusCode::NOT_FOUND,
                "Resource not found",
            ),
            (
                Method::GET,
                "/items/1",
                "",
                StatusCode::METHOD_NOT_ALLOWED,
                "Method not allowed",
            ),
        ] {
            assert_error_response(rejection_response(method, uri, body).await, status, message)
                .await;
        }

        let unsupported_media = rejection_test_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/items/1")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_error_response(
            unsupported_media,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Unsupported media type",
        )
        .await;
    }

    #[tokio::test]
    async fn normalization_preserves_method_headers() {
        let response = rejection_response(Method::GET, "/items/1", "").await;
        assert_eq!(response.headers().get(header::ALLOW).unwrap(), "POST");
        assert_error_response(
            response,
            StatusCode::METHOD_NOT_ALLOWED,
            "Method not allowed",
        )
        .await;
    }

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
            ("ResearchPaper", "doi"),
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
