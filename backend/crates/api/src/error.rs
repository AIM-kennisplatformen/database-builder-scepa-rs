use axum::{
    Json,
    body::Body,
    http::{HeaderValue, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use scepa::{
    conflict::Conflict,
    document_upload::DocumentUploadError,
    models::canonical::CanonicalMissingField,
    postgres::ReviewCase,
    restate::{RestateError, RestateErrorKind},
};
use serde::Serialize;
use utoipa::ToSchema;

use crate::endpoints::AppState;

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct DocumentValidationErrorResponse {
    pub(crate) error: String,
    pub(crate) action: String,
    pub(crate) code: String,
    pub(crate) workflow_id: String,
    pub(crate) pdf_hash: String,
    pub(crate) review_case_id: i64,
    pub(crate) missing_fields: Vec<CanonicalMissingField>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ErrorResponse {
    pub(crate) error: String,
    pub(crate) action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) workflow_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) pdf_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) review_case_id: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum UploadErrorResponse {
    General(ErrorResponse),
    Validation(DocumentValidationErrorResponse),
}

#[derive(Debug)]
pub(crate) struct ApiError(pub(crate) StatusCode, pub(crate) UploadErrorResponse);

impl ApiError {
    pub(crate) fn validation(validation: DocumentValidationErrorResponse) -> Self {
        Self(
            StatusCode::UNPROCESSABLE_ENTITY,
            UploadErrorResponse::Validation(validation),
        )
    }

    fn general_mut(&mut self) -> &mut ErrorResponse {
        match &mut self.1 {
            UploadErrorResponse::General(body) => body,
            UploadErrorResponse::Validation(_) => {
                unreachable!("validation errors have no general body")
            }
        }
    }

    pub(crate) fn with_pdf_hash(mut self, pdf_hash: String) -> Self {
        self.general_mut().pdf_hash = Some(pdf_hash);
        self
    }

    pub(crate) fn with_review_case_id(mut self, review_case_id: i64) -> Self {
        self.general_mut().review_case_id = Some(review_case_id);
        self
    }
}

#[derive(Clone, Copy)]
pub(crate) struct StructuredApiError;

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (self.0, Json(self.1)).into_response();
        response.extensions_mut().insert(StructuredApiError);
        response
    }
}

pub(crate) async fn normalize_error_response(request: Request<Body>, next: Next) -> Response {
    normalize_framework_error(next.run(request).await)
}

pub(crate) fn normalize_framework_error(mut response: Response) -> Response {
    let status = response.status();
    if !(status.is_client_error() || status.is_server_error())
        || response.extensions().get::<StructuredApiError>().is_some()
    {
        return response;
    }

    let (message, action) = match status {
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => {
            ("Invalid request", "Correct the request data and try again")
        }
        StatusCode::NOT_FOUND => ("Resource not found", "Refresh the page and try again"),
        StatusCode::METHOD_NOT_ALLOWED => (
            "Method not allowed",
            "Use one of the methods allowed by this endpoint",
        ),
        StatusCode::PAYLOAD_TOO_LARGE => (
            "Request body too large",
            "Submit a file smaller than 64 MiB",
        ),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => (
            "Unsupported media type",
            "Submit the request using the documented content type",
        ),
        StatusCode::BAD_GATEWAY => (
            "An upstream service returned an invalid response",
            "Try again; if the problem continues, contact support",
        ),
        StatusCode::SERVICE_UNAVAILABLE => (
            "A required service is temporarily unavailable",
            "Wait a moment and try again",
        ),
        StatusCode::GATEWAY_TIMEOUT => (
            "A required service timed out",
            "Try again; if the problem continues, contact support",
        ),
        status if status.is_client_error() => {
            ("Request failed", "Correct the request and try again")
        }
        _ => (
            "Internal server error",
            "Try again; if the problem continues, contact support",
        ),
    };
    let json_response = Json(ErrorResponse {
        error: message.into(),
        action: action.into(),
        workflow_id: None,
        pdf_hash: None,
        review_case_id: None,
    })
    .into_response();

    *response.body_mut() = json_response.into_body();
    response.headers_mut().remove(header::CONTENT_LENGTH);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

pub(crate) fn internal(error: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %error, "API request failed internally");
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Internal server error",
        "Try again; if the problem continues, contact support",
    )
}

pub(crate) fn api_error(
    status: StatusCode,
    error: impl Into<String>,
    action: impl Into<String>,
) -> ApiError {
    ApiError(
        status,
        UploadErrorResponse::General(ErrorResponse {
            error: error.into(),
            action: action.into(),
            workflow_id: None,
            pdf_hash: None,
            review_case_id: None,
        }),
    )
}

pub(crate) async fn document_upload_error(
    state: &AppState,
    workflow_id: &str,
    error: DocumentUploadError,
    pdf_hash: Option<String>,
) -> ApiError {
    match error {
        DocumentUploadError::InvalidInput(message) => {
            let mut error = api_error(
                StatusCode::BAD_REQUEST,
                message,
                "Provide a non-empty workflow identifier and try again",
            );
            error.general_mut().workflow_id =
                (!workflow_id.is_empty()).then(|| workflow_id.to_owned());
            error
        }
        DocumentUploadError::Storage(source) => {
            let conflict = source
                .get_ref()
                .and_then(|error| error.downcast_ref::<Conflict>())
                .copied();
            let case = if conflict.is_some() {
                None
            } else {
                tracing::error!(error = %source, %workflow_id, "PDF storage pipeline failed");
                latest_review_case(state, workflow_id).await
            };
            let mut error = classify_storage_error(conflict, case.as_ref());
            add_error_context(&mut error, workflow_id, pdf_hash, case.as_ref());
            error
        }
        DocumentUploadError::Workflow(error) => {
            restate_api_error(state, error, pdf_hash, None).await
        }
    }
}

fn classify_storage_error(conflict: Option<Conflict>, case: Option<&ReviewCase>) -> ApiError {
    if let Some(conflict) = conflict {
        let action = if conflict == Conflict::DuplicatePdf {
            "Open the existing document to review or update it"
        } else {
            "Use a new workflow identifier or refresh the existing submission"
        };
        return api_error(StatusCode::CONFLICT, conflict.to_string(), action);
    }

    if case.is_some_and(|case| case.service == "garage" && case.phase == "input_validation") {
        api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "The uploaded file is not a valid PDF",
            "Choose a non-empty PDF file and upload it again",
        )
    } else if case.is_some() {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The PDF could not be stored correctly",
            "Try again; if the problem continues, contact support",
        )
    } else {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Document storage is temporarily unavailable",
            "Wait a moment and upload the PDF again",
        )
    }
}

pub(crate) async fn restate_api_error(
    state: &AppState,
    error: RestateError,
    pdf_hash: Option<String>,
    review_case_id: Option<i64>,
) -> ApiError {
    tracing::error!(error = %error, "API request failed through Restate");
    let workflow_id = error.workflow_id().to_owned();
    let mut api_error = classify_restate_error(error.kind(), error.rejection_message());

    let case = latest_review_case(state, &workflow_id).await;
    add_error_context(&mut api_error, &workflow_id, pdf_hash, case.as_ref());
    if api_error.general_mut().review_case_id.is_none() {
        api_error.general_mut().review_case_id = review_case_id;
    }
    api_error
}

pub(crate) fn classify_restate_error(
    kind: RestateErrorKind,
    rejection_message: Option<&str>,
) -> ApiError {
    match kind {
        RestateErrorKind::InvalidRequest => api_error(
            StatusCode::BAD_REQUEST,
            "The workflow request is invalid",
            "Correct the request identifiers and try again",
        ),
        RestateErrorKind::Unavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Document processing is temporarily unavailable",
            "Wait a moment and try again",
        ),
        RestateErrorKind::Timeout => api_error(
            StatusCode::GATEWAY_TIMEOUT,
            "Document processing timed out",
            "Try again; if the problem continues, contact support",
        ),
        RestateErrorKind::InvalidResponse => api_error(
            StatusCode::BAD_GATEWAY,
            "Document processing returned an invalid response",
            "Try again; if the problem continues, contact support",
        ),
        RestateErrorKind::Rejected(404) => api_error(
            StatusCode::NOT_FOUND,
            "A required document artifact was not found",
            "Upload the document again or refresh the document list",
        ),
        RestateErrorKind::Rejected(409) => {
            let message = rejection_message
                .and_then(scepa::conflict::Conflict::from_message)
                .map(|conflict| conflict.to_string())
                .unwrap_or_else(|| "The workflow conflicts with existing data".into());
            api_error(
                StatusCode::CONFLICT,
                message,
                "Refresh the current data or use a new workflow identifier",
            )
        }
        RestateErrorKind::Rejected(422) => {
            let enrichment = rejection_message == Some("External enrichment is not available");
            api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                if enrichment {
                    "External enrichment is not available"
                } else {
                    "The document could not be processed"
                },
                if enrichment {
                    "Submit the repair again with enrichment disabled"
                } else {
                    "Review the document and correct its contents before trying again"
                },
            )
        }
        RestateErrorKind::Rejected(429 | 503) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Document processing is temporarily unavailable",
            "Wait a moment and try again",
        ),
        RestateErrorKind::Rejected(408 | 504) => api_error(
            StatusCode::GATEWAY_TIMEOUT,
            "Document processing timed out",
            "Try again; if the problem continues, contact support",
        ),
        RestateErrorKind::Rejected(500) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Document processing failed",
            "Try again; if the problem continues, contact support",
        ),
        RestateErrorKind::Rejected(status) if (400..500).contains(&status) => api_error(
            StatusCode::BAD_REQUEST,
            "The workflow request was rejected",
            "Correct the request and try again",
        ),
        RestateErrorKind::Rejected(_) => api_error(
            StatusCode::BAD_GATEWAY,
            "Document processing returned an unexpected error",
            "Try again; if the problem continues, contact support",
        ),
    }
}

async fn latest_review_case(state: &AppState, workflow_id: &str) -> Option<ReviewCase> {
    match state
        .drafts
        .get_latest_pending_case_for_workflow(workflow_id)
        .await
    {
        Ok(case) => case,
        Err(error) => {
            tracing::error!(error = %error, %workflow_id, "could not load workflow review context");
            None
        }
    }
}

fn add_error_context(
    error: &mut ApiError,
    workflow_id: &str,
    pdf_hash: Option<String>,
    case: Option<&ReviewCase>,
) {
    let body = error.general_mut();
    body.workflow_id = Some(workflow_id.to_owned());
    body.pdf_hash = case.and_then(|case| case.pdf_hash.clone()).or(pdf_hash);
    body.review_case_id = case.map(|case| case.id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::to_bytes,
        extract::{DefaultBodyLimit, Path},
        http::{Method, Request},
        middleware,
        routing::post,
    };
    use serde_json::Value;
    use tower::ServiceExt;
    async fn assert_error_response(response: Response, status: StatusCode, message: &str) {
        assert_eq!(response.status(), status);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = serde_json::from_slice::<Value>(&body).unwrap();
        assert_eq!(body["error"], message);
        assert!(
            body["action"]
                .as_str()
                .is_some_and(|action| !action.is_empty())
        );
    }

    fn rejection_test_router() -> Router {
        async fn accept_json(Path(_id): Path<u64>, Json(_body): Json<Value>) -> StatusCode {
            StatusCode::NO_CONTENT
        }

        Router::new()
            .route("/items/{id}", post(accept_json))
            .layer(DefaultBodyLimit::max(16))
            .layer(middleware::from_fn(normalize_error_response))
    }

    async fn rejection_response(method: Method, uri: &str, body: &str) -> Response {
        rejection_test_router()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_owned()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn conflicts_reach_http_as_safe_json() {
        for conflict in [
            scepa::conflict::Conflict::DuplicatePdf,
            scepa::conflict::Conflict::WorkflowPdf,
            scepa::conflict::Conflict::Record,
            scepa::conflict::Conflict::CanonicalIdentity,
            scepa::conflict::Conflict::PassageIdentity,
            scepa::conflict::Conflict::Submission,
        ] {
            assert_error_response(
                normalize_framework_error(
                    api_error(
                        StatusCode::CONFLICT,
                        conflict.to_string(),
                        "Refresh the current data or use a new workflow identifier",
                    )
                    .into_response(),
                ),
                StatusCode::CONFLICT,
                &conflict.to_string(),
            )
            .await;
        }
    }

    #[tokio::test]
    async fn api_errors_are_json_objects_with_safe_messages() {
        assert_error_response(
            api_error(
                StatusCode::NOT_FOUND,
                "PDF not found",
                "Upload the PDF again",
            )
            .into_response(),
            StatusCode::NOT_FOUND,
            "PDF not found",
        )
        .await;
        assert_error_response(
            internal("database password leaked").into_response(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal server error",
        )
        .await;
    }

    #[test]
    fn storage_failures_use_semantic_statuses() {
        let mut case = ReviewCase {
            id: 42,
            workflow_id: "upload:workflow-1".into(),
            pdf_hash: None,
            service: "garage".into(),
            phase: "input_validation".into(),
            retryable: false,
            error_message: String::new(),
            artifact_content_type: String::new(),
            artifact_size: 0,
            status: String::new(),
            resolution: None,
            created_at: String::new(),
            resolved_at: None,
        };

        assert_eq!(
            classify_storage_error(Some(Conflict::DuplicatePdf), None).0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            classify_storage_error(None, Some(&case)).0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        case.phase = "storage".into();
        assert_eq!(
            classify_storage_error(None, Some(&case)).0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            classify_storage_error(None, None).0,
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[test]
    fn restate_failures_use_semantic_statuses_without_leaking_rejection_details() {
        for (kind, expected) in [
            (RestateErrorKind::InvalidRequest, StatusCode::BAD_REQUEST),
            (RestateErrorKind::Rejected(404), StatusCode::NOT_FOUND),
            (RestateErrorKind::Rejected(409), StatusCode::CONFLICT),
            (
                RestateErrorKind::Rejected(422),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                RestateErrorKind::Rejected(500),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                RestateErrorKind::Rejected(429),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (RestateErrorKind::Rejected(408), StatusCode::GATEWAY_TIMEOUT),
            (RestateErrorKind::InvalidResponse, StatusCode::BAD_GATEWAY),
            (
                RestateErrorKind::Unavailable,
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (RestateErrorKind::Timeout, StatusCode::GATEWAY_TIMEOUT),
        ] {
            let error = classify_restate_error(kind, Some("database password leaked"));
            assert_eq!(error.0, expected);
            let body = serde_json::to_string(&error.1).unwrap();
            assert!(!body.contains("database password leaked"));
            let body: Value = serde_json::from_str(&body).unwrap();
            assert!(
                body["action"]
                    .as_str()
                    .is_some_and(|action| !action.is_empty())
            );
        }
    }

    #[tokio::test]
    async fn framework_rejections_are_normalized_as_json() {
        for (method, uri, body, status, message) in [
            (
                Method::POST,
                "/items/1",
                "{",
                StatusCode::BAD_REQUEST,
                "Invalid request",
            ),
            (
                Method::POST,
                "/items/not-a-number",
                "{}",
                StatusCode::BAD_REQUEST,
                "Invalid request",
            ),
            (
                Method::POST,
                "/items/1",
                r#"{"value":"too large"}"#,
                StatusCode::PAYLOAD_TOO_LARGE,
                "Request body too large",
            ),
            (
                Method::GET,
                "/missing",
                "",
                StatusCode::NOT_FOUND,
                "Resource not found",
            ),
            (
                Method::GET,
                "/items/1",
                "",
                StatusCode::METHOD_NOT_ALLOWED,
                "Method not allowed",
            ),
        ] {
            assert_error_response(rejection_response(method, uri, body).await, status, message)
                .await;
        }

        let unsupported_media = rejection_test_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/items/1")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_error_response(
            unsupported_media,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Unsupported media type",
        )
        .await;
    }

    #[tokio::test]
    async fn normalization_preserves_method_headers() {
        let response = rejection_response(Method::GET, "/items/1", "").await;
        assert_eq!(response.headers().get(header::ALLOW).unwrap(), "POST");
        assert_error_response(
            response,
            StatusCode::METHOD_NOT_ALLOWED,
            "Method not allowed",
        )
        .await;
    }
}
