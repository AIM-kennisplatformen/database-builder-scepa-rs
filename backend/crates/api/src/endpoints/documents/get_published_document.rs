use super::super::*;

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
pub(crate) async fn get_published_document(
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
