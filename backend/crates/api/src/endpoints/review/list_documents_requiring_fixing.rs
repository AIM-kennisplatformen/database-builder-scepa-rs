use super::super::*;

#[utoipa::path(
    get,
    path = "/documents/requiring-fixing",
    responses(
        (status = 200, description = "Pending review cases with document summaries", body = Vec<ReviewCaseDocumentSummary>),
        (status = 500, description = "Persistence error", body = ErrorResponse)
    ),
    tag = "review"
)]
pub(crate) async fn list_documents_requiring_fixing(
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
