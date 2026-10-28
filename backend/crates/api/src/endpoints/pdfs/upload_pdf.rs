use super::super::*;

#[utoipa::path(
    post,
    path = "/pdfs",
    request_body(content = Vec<u8>, content_type = "application/pdf", description = "PDF document"),
    responses(
        (status = 201, description = "Document processed", body = UploadResponse),
        (status = 413, description = "PDF exceeds the upload limit", body = ErrorResponse),
        (status = 422, description = "PDF or extracted document requires correction", body = UploadErrorResponse),
        (status = 409, description = "Conflicting data or workflow identity", body = ErrorResponse),
        (status = 404, description = "Stored workflow artifact not found", body = ErrorResponse),
        (status = 500, description = "Document processing failed", body = ErrorResponse),
        (status = 502, description = "Invalid upstream response", body = ErrorResponse),
        (status = 503, description = "Document processing unavailable", body = ErrorResponse),
        (status = 504, description = "Document processing timed out", body = ErrorResponse)
    ),
    tag = "documents"
)]
pub(crate) async fn upload_pdf(
    State(state): State<AppState>,
    pdf: Bytes,
) -> Result<Response, ApiError> {
    let pdf = pdf.to_vec();
    let pdf_hash = sha256_hex(&pdf);
    let upload = match state.uploads.run(pdf).await {
        Ok(upload) => upload,
        Err(failure) => {
            if let Some(validation) =
                upload_validation_response(&state, &failure.workflow_id).await?
            {
                return Err(ApiError::validation(validation));
            }
            return Err(document_upload_error(
                &state,
                &failure.workflow_id,
                failure.source,
                Some(pdf_hash),
            )
            .await);
        }
    };

    Ok((
        StatusCode::CREATED,
        Json(UploadResponse {
            workflow_id: upload.workflow_id,
            result: upload.result,
        }),
    )
        .into_response())
}

async fn upload_validation_response(
    state: &AppState,
    workflow_id: &str,
) -> Result<Option<DocumentValidationErrorResponse>, ApiError> {
    let Some(case) = state
        .drafts
        .get_pending_case_for_workflow(workflow_id, "typedb", "input_validation")
        .await
        .map_err(internal)?
        .filter(|case| !case.retryable)
    else {
        return Ok(None);
    };
    let Some(pdf_hash) = case.pdf_hash.clone() else {
        return Ok(None);
    };
    let draft = state
        .drafts
        .get_repair_draft(&pdf_hash)
        .await
        .map_err(internal)?;
    Ok(document_validation_response(
        workflow_id,
        case.id,
        pdf_hash,
        &draft,
    ))
}

pub(crate) fn document_validation_response(
    workflow_id: &str,
    review_case_id: i64,
    pdf_hash: String,
    draft: &DraftDocument,
) -> Option<DocumentValidationErrorResponse> {
    let missing_fields = canonical_missing_fields(&draft.effective_document());
    if missing_fields.is_empty() {
        return None;
    }

    Some(DocumentValidationErrorResponse {
        error: "Document requires correction".into(),
        action: "Open the review case and supply the missing document fields".into(),
        code: "document_validation_failed".into(),
        workflow_id: workflow_id.into(),
        pdf_hash,
        review_case_id,
        missing_fields,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::normalize_framework_error;
    use axum::body::to_bytes;
    use scepa::models::draft::{Bibliography, PassageLevel, TeiDocument};
    use serde_json::Value;
    #[tokio::test]
    async fn upload_validation_response_survives_http_error_normalization() {
        let draft = DraftDocument::new(TeiDocument {
            id: String::new(),
            level: PassageLevel::Paragraph,
            bibliography: Bibliography::default(),
            body_text: Vec::new(),
            figures_and_tables: Vec::new(),
        });

        let response =
            document_validation_response("upload:workflow-1", 42, "a".repeat(64), &draft).unwrap();
        assert_eq!(response.code, "document_validation_failed");
        assert!(!response.action.is_empty());
        assert_eq!(response.workflow_id, "upload:workflow-1");
        assert_eq!(response.review_case_id, 42);
        assert_eq!(
            response
                .missing_fields
                .iter()
                .map(|field| field.path.as_str())
                .collect::<Vec<_>>(),
            vec!["bibliography.title", "bibliography.authors"]
        );

        let normalized = normalize_framework_error(ApiError::validation(response).into_response());
        assert_eq!(normalized.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = to_bytes(normalized.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["code"], "document_validation_failed");
        assert_eq!(body["review_case_id"], 42);
        assert_eq!(body["missing_fields"].as_array().unwrap().len(), 2);
    }
}
