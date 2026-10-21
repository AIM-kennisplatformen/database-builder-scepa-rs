use super::super::*;

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
pub(crate) async fn get_document_requiring_fixing(
    State(state): State<AppState>,
    Path(case_id): Path<i64>,
) -> Result<Json<RepairDraftResponse>, ApiError> {
    let repair = load_repair(&state.drafts, case_id).await?;
    let missing_fields = canonical_missing_fields(&repair.draft.effective_document());

    Ok(Json(RepairDraftResponse {
        case: repair.case,
        draft: DraftResponse {
            pdf_hash: repair.pdf_hash,
            draft: repair.draft,
        },
        missing_fields,
    }))
}
