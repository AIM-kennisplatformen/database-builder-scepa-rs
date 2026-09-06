//! Public Axum API for uploading new documents.

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderValue, Request, StatusCode, header},
    middleware::{self, Next},
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

#[derive(Serialize, ToSchema)]
struct ErrorResponse {
    error: String,
}

struct ApiError(StatusCode, String);

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
        let mut response = (self.0, Json(ErrorResponse { error: self.1 })).into_response();
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

    let message = match status {
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => "Invalid request",
        StatusCode::NOT_FOUND => "Resource not found",
        StatusCode::METHOD_NOT_ALLOWED => "Method not allowed",
        StatusCode::PAYLOAD_TOO_LARGE => "Request body too large",
        StatusCode::UNSUPPORTED_MEDIA_TYPE => "Unsupported media type",
        StatusCode::BAD_GATEWAY => "Upstream service unavailable",
        status if status.is_client_error() => "Request failed",
        _ => "Internal server error",
    };
    let json_response = Json(ErrorResponse {
        error: message.into(),
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
        (status = 413, description = "PDF exceeds the upload limit", body = ErrorResponse),
        (status = 502, description = "Pipeline or workflow error", body = ErrorResponse)
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
        (status = 400, description = "Invalid submission", body = ErrorResponse),
        (status = 413, description = "PDF exceeds the upload limit", body = ErrorResponse),
        (status = 502, description = "Pipeline or workflow error", body = ErrorResponse)
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
        (status = 400, description = "Invalid request body", body = ErrorResponse),
        (status = 415, description = "Unsupported media type", body = ErrorResponse),
        (status = 422, description = "Invalid request data", body = ErrorResponse),
        (status = 502, description = "Workflow error", body = ErrorResponse)
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
        (status = 200, description = "Pending review cases", body = Vec<scepa::postgres::ReviewCase>),
        (status = 500, description = "Persistence error", body = ErrorResponse)
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
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 415, description = "Unsupported media type", body = ErrorResponse),
        (status = 422, description = "Invalid request data", body = ErrorResponse),
        (status = 502, description = "Workflow error", body = ErrorResponse)
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
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "published document not found".into()))
}

#[utoipa::path(
    put,
    path = "/documents/{pdf_hash}",
    params(("pdf_hash" = String, Path, description = "SHA-256 hash of the PDF")),
    request_body(content = ManualDocument, description = "Manual document corrections"),
    responses(
        (status = 200, description = "Updated document", body = UpdateDocumentWorkflowResponse),
        (status = 400, description = "Invalid request body", body = ErrorResponse),
        (status = 415, description = "Unsupported media type", body = ErrorResponse),
        (status = 422, description = "Invalid request data", body = ErrorResponse),
        (status = 502, description = "Workflow error", body = ErrorResponse)
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
    tracing::error!(error = %error, "API request failed internally");
    ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Internal server error".into(),
    )
}

fn upstream(error: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %error, "API request failed through an upstream service");
    ApiError(
        StatusCode::BAD_GATEWAY,
        "Upstream service unavailable".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::to_bytes,
        http::{Method, Request},
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    async fn assert_error_response(response: Response, status: StatusCode, message: &str) {
        assert_eq!(response.status(), status);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({
                "error": message
            })
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
    async fn api_errors_are_json_objects_with_safe_messages() {
        assert_error_response(
            ApiError(StatusCode::NOT_FOUND, "PDF not found".into()).into_response(),
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
        assert_error_response(
            upstream("workflow internals leaked").into_response(),
            StatusCode::BAD_GATEWAY,
            "Upstream service unavailable",
        )
        .await;
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

        assert!(
            schemas["ErrorResponse"]["properties"]
                .get("error")
                .is_some(),
            "the API error schema must expose a human-readable error message"
        );

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
                    assert_eq!(
                        content["application/json"]["schema"]["$ref"],
                        "#/components/schemas/ErrorResponse",
                        "error response {status} does not use ErrorResponse"
                    );
                }
            }
        }
    }
}
