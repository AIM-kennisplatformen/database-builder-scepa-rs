use super::super::*;

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
pub(crate) async fn publish_draft(
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
