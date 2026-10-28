use super::super::*;

#[utoipa::path(
    get,
    path = "/documents",
    responses(
        (status = 200, description = "Published document summaries", body = Vec<PublishedDocumentSummary>),
        (status = 500, description = "Persistence error", body = ErrorResponse)
    ),
    tag = "documents"
)]
pub(crate) async fn list_documents(
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
