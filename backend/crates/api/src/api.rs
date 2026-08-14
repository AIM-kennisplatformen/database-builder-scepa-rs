//! Public Axum API for uploading new documents.

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use scepa::{
    orchestration::{NewDocumentIngressClient, NewDocumentWorkflowResponse},
    pipeline::{
        PipelineService,
        garage::{GaragePipelineService, StoredPdf, sha256_hex},
    },
};
use serde::Serialize;
use tower_http::trace::TraceLayer;

const MAX_PDF_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    workflows: NewDocumentIngressClient,
    pdfs: GaragePipelineService,
}

impl AppState {
    pub fn new(workflows: NewDocumentIngressClient, pdfs: GaragePipelineService) -> Self {
        Self { workflows, pdfs }
    }
}

#[derive(Serialize)]
struct UploadResponse {
    workflow_id: String,
    result: NewDocumentWorkflowResponse,
}

#[derive(Serialize)]
struct SubmissionResponse {
    workflow_id: String,
}

struct UploadError(String);

impl IntoResponse for UploadError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_GATEWAY, self.0).into_response()
    }
}

/// Builds the document-upload HTTP router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/pdfs", post(upload_pdf))
        .route("/pdfs/submissions/{workflow_id}", post(submit_pdf))
        .layer(DefaultBodyLimit::max(MAX_PDF_BYTES))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn upload_pdf(
    State(state): State<AppState>,
    pdf: Bytes,
) -> Result<(StatusCode, Json<UploadResponse>), UploadError> {
    let workflow_id = sha256_hex(&pdf);
    let stored = store_pdf(&state, &workflow_id, pdf).await?;
    let result = state
        .workflows
        .run(&workflow_id, stored.pdf_hash)
        .await
        .map_err(|error| UploadError(error.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(UploadResponse {
            workflow_id,
            result,
        }),
    ))
}

async fn submit_pdf(
    State(state): State<AppState>,
    Path(workflow_id): Path<String>,
    pdf: Bytes,
) -> Result<(StatusCode, Json<SubmissionResponse>), UploadError> {
    if workflow_id.is_empty() {
        return Err(UploadError("workflow identifier must not be empty".into()));
    }

    let stored = store_pdf(&state, &workflow_id, pdf).await?;
    state
        .workflows
        .submit(&workflow_id, stored.pdf_hash)
        .await
        .map_err(|error| UploadError(error.to_string()))?;

    Ok((
        StatusCode::ACCEPTED,
        Json(SubmissionResponse { workflow_id }),
    ))
}

async fn store_pdf(
    state: &AppState,
    workflow_id: &str,
    pdf: Bytes,
) -> Result<StoredPdf, UploadError> {
    state
        .pdfs
        .execute(workflow_id, &pdf.to_vec())
        .await
        .map(|outcome| outcome.into_output(|_| {}))
        .map_err(|error| UploadError(error.to_string()))
}
