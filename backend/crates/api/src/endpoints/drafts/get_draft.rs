use super::super::*;

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
pub(crate) async fn get_draft(
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
