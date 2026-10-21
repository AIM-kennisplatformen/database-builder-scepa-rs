use super::super::*;

#[utoipa::path(
    put,
    path = "/documents/requiring-fixing/{case_id}",
    params(
        ("case_id" = i64, Path, description = "Review case identifier"),
        ("Idempotency-Key" = Option<String>, Header, description = "Required when the request creates objects without IDs")
    ),
    request_body(content = RepairDocumentRequest),
    responses(
        (status = 200, description = "Repaired and published document", body = PublishResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
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
    tag = "review"
)]
pub(crate) async fn fix_document(
    State(state): State<AppState>,
    Path(case_id): Path<i64>,
    headers: HeaderMap,
    Json(mut request): Json<RepairDocumentRequest>,
) -> Result<Json<PublishResponse>, ApiError> {
    let repair = load_repair(&state.drafts, case_id).await?;
    assign_request_ids(&repair.pdf_hash, &headers, &mut request.manual_data)?;
    validate_artifact_update(repair.draft, &request.manual_data)?;
    let result = match state
        .restate
        .fix_document(FixDocumentWorkflowRequest {
            case_id,
            manual_data: request.manual_data,
            enrich: request.enrich,
        })
        .await
    {
        Ok(result) => result,
        Err(error) => {
            return Err(restate_api_error(
                &state,
                error,
                Some(repair.pdf_hash.clone()),
                Some(case_id),
            )
            .await);
        }
    };

    Ok(Json(PublishResponse {
        artifact: DraftResponse {
            pdf_hash: result.pdf_hash,
            draft: result.artifact,
        },
        canonical: result.canonical,
    }))
}
