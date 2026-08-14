//! Public Axum API for uploading new documents.

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Serialize;
use tower_http::trace::TraceLayer;

use scepa::{
    operations::{OperationError, submit_pipeline},
    pipeline::{
        PipelineService,
        garage::{GaragePipelineService, StoredPdf, sha256_hex},
    },
};

const MAX_PDF_BYTES: usize = 30 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub garage_pipeline: GaragePipelineService,
    pub http_client: reqwest::Client,
    pub restate_ingress_url: String,
}

/// Builds the document-upload HTTP router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/pdfs", post(upload_pdf))
        .layer(DefaultBodyLimit::max(MAX_PDF_BYTES))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[derive(Serialize)]
struct UploadPdfResponse {
    workflow_id: String,
    pdf: StoredPdf,
    submission_status: u16,
}

async fn upload_pdf(
    State(state): State<AppState>,
    pdf: Bytes,
) -> Result<(StatusCode, Json<UploadPdfResponse>), ApiError> {
    validate_pdf(&pdf)?;
    let stored = state
        .garage_pipeline
        .execute(&sha256_hex(&pdf), &pdf.to_vec())
        .await
        .map_err(ApiError::storage)?
        .into_output(|_| {});
    let workflow_id = stored.pdf_hash.clone();
    state
        .garage_pipeline
        .metadata()
        .link_workflow(&workflow_id, &stored.pdf_hash)
        .await
        .map_err(ApiError::internal)?;
    let response = submit_pipeline(
        &state.http_client,
        &state.restate_ingress_url,
        &workflow_id,
        &stored.pdf_hash,
    )
    .await
    .map_err(ApiError::from_operation)?;
    let submission_status = response.status().as_u16();
    if !response.status().is_success() {
        return Err(restate_submission_error(response).await);
    }

    Ok((
        StatusCode::ACCEPTED,
        Json(UploadPdfResponse {
            workflow_id,
            pdf: stored,
            submission_status,
        }),
    ))
}

fn validate_pdf(pdf: &[u8]) -> Result<(), ApiError> {
    if pdf.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "request body must contain a PDF",
        ));
    }
    if !pdf.starts_with(b"%PDF-") {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "request body is not a PDF file",
        ));
    }
    Ok(())
}

async fn restate_submission_error(response: reqwest::Response) -> ApiError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    tracing::warn!(%status, %body, "Restate pipeline submission failed");
    ApiError::new(
        StatusCode::BAD_GATEWAY,
        "Restate rejected pipeline submission",
    )
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(%error, "API operation failed");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
    }

    fn storage(error: impl std::fmt::Display) -> Self {
        tracing::warn!(%error, "PDF storage operation failed");
        Self::new(StatusCode::BAD_GATEWAY, "PDF storage is unavailable")
    }

    fn from_operation(error: OperationError) -> Self {
        match error {
            OperationError::NotFound(message) => Self::new(StatusCode::NOT_FOUND, message),
            OperationError::Invalid(message) => {
                Self::new(StatusCode::UNPROCESSABLE_ENTITY, message)
            }
            OperationError::Internal(message) => {
                tracing::error!(%message, "pipeline submission failed");
                Self::new(StatusCode::BAD_GATEWAY, message)
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct ErrorBody {
            error: String,
        }

        (
            self.status,
            Json(ErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_documents() {
        let error = validate_pdf(&[]).unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn rejects_non_pdf_documents() {
        let error = validate_pdf(b"not a pdf").unwrap_err();
        assert_eq!(error.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[test]
    fn accepts_pdf_documents() {
        assert!(validate_pdf(b"%PDF-1.7\n").is_ok());
    }
}
