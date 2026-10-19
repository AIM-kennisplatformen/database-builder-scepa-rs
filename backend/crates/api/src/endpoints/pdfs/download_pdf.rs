use super::super::*;

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
pub(crate) async fn download_pdf(
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
