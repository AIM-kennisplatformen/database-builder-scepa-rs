use super::super::*;

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
pub(crate) async fn submit_pdf(
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
