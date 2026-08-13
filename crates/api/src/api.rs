//! Public Axum API for workflow submission and operator review.

use std::path::PathBuf;

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;

use scepa::{
    pipeline::{
        PipelineService,
        garage::{GaragePipelineService, StoredPdf, sha256_hex},
        typedb::{TypeDbService, TypeDbStore},
    },
    postgres::{PostgresReviewStore, ReviewCase},
    restate::{ResolveReviewRequest, ReviewDecision, delete_debug_artifacts},
};

use scepa::operations::{
    OperationError, PipelinePart, restate_url, run_artifact_operation, submit_pipeline,
};

const MAX_PDF_BYTES: usize = 30 * 1024 * 1024;
const MAX_BATCH_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub review_store: PostgresReviewStore,
    pub typedb: TypeDbService<TypeDbStore>,
    pub garage_pipeline: GaragePipelineService,
    pub http_client: reqwest::Client,
    pub restate_ingress_url: String,
    pub restate_admin_url: String,
    pub grobid_url: String,
    pub debug_artifact_root: PathBuf,
}

/// Builds the public HTTP router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/pdfs", post(upload_pdf))
        .route("/pdfs/{pdf_hash}", get(download_pdf))
        .route("/workflows/{workflow_id}", post(submit_workflow))
        .route("/workflows/{workflow_id}/output", get(workflow_output))
        .route(
            "/pipeline/grobid/{part}/{identifier}",
            post(run_stored_pipeline_part).patch(patch_and_run_stored_pipeline_part),
        )
        .route("/pipeline/run/{identifier}", post(run_pipeline))
        .route("/pipeline/run/batch", post(run_pipeline_batch))
        .route(
            "/pipeline/artifacts/{identifier}",
            delete(delete_pdf_knowledge),
        )
        .route("/review-cases", get(list_review_cases))
        .route("/review-cases/count", get(count_pending_review_cases))
        .route("/review-cases/{id}", get(get_review_case))
        .route(
            "/review-cases/{id}/artifact",
            get(get_review_artifact).patch(patch_review_artifact),
        )
        .route("/review-cases/{id}/resolve", post(resolve_review_case))
        .layer(DefaultBodyLimit::max(MAX_BATCH_BYTES))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[derive(Serialize)]
struct DeletePdfResponse {
    identifier: String,
    purged_workflows: usize,
    deleted_review_cases: u64,
    deleted_artifacts: usize,
    unlinked_pdf: bool,
}

async fn delete_pdf_knowledge(
    State(state): State<AppState>,
    Path(identifier): Path<String>,
) -> Result<Json<DeletePdfResponse>, ApiError> {
    let purged_workflows = purge_restate_workflow(&state, &identifier).await?;
    let deleted_review_cases = state
        .review_store
        .delete_cases_for_workflow(&identifier)
        .await
        .map_err(ApiError::internal)?;
    let deleted_artifacts = delete_debug_artifacts(&state.debug_artifact_root, &identifier)
        .await
        .map_err(ApiError::internal)?;
    let unlinked_pdf = state
        .garage_pipeline
        .metadata()
        .unlink_workflow(&identifier)
        .await
        .map_err(ApiError::internal)?;

    Ok(Json(DeletePdfResponse {
        identifier,
        purged_workflows,
        deleted_review_cases,
        deleted_artifacts,
        unlinked_pdf,
    }))
}

#[derive(Serialize)]
struct RestateQuery<'a> {
    query: &'a str,
}

async fn purge_restate_workflow(state: &AppState, identifier: &str) -> Result<usize, ApiError> {
    let escaped_identifier = identifier.replace('\'', "''");
    let query = format!(
        "SELECT id FROM sys_invocation \
         WHERE target_service_name = 'ScepaPipeline' \
         AND target_service_key = '{escaped_identifier}' \
         AND target_handler_name = 'run'"
    );
    let query_url =
        restate_url(&state.restate_admin_url, &["query"]).map_err(ApiError::from_operation)?;
    let response = state
        .http_client
        .post(query_url)
        .header(header::ACCEPT, "application/json")
        .json(&RestateQuery { query: &query })
        .send()
        .await
        .map_err(ApiError::bad_gateway)?;
    if !response.status().is_success() {
        return Err(restate_admin_error("query", response).await);
    }
    let value: serde_json::Value = response.json().await.map_err(ApiError::bad_gateway)?;
    let mut invocation_ids = Vec::new();
    collect_invocation_ids(&value, &mut invocation_ids);
    invocation_ids.sort_unstable();
    invocation_ids.dedup();

    for invocation_id in &invocation_ids {
        purge_restate_invocation(state, invocation_id).await?;
    }
    Ok(invocation_ids.len())
}

fn collect_invocation_ids(value: &serde_json::Value, invocation_ids: &mut Vec<String>) {
    match value {
        serde_json::Value::String(value) if value.starts_with("inv_") => {
            invocation_ids.push(value.clone());
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_invocation_ids(value, invocation_ids);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                collect_invocation_ids(value, invocation_ids);
            }
        }
        _ => {}
    }
}

async fn purge_restate_invocation(state: &AppState, invocation_id: &str) -> Result<(), ApiError> {
    let purge_url = restate_url(
        &state.restate_admin_url,
        &["invocations", invocation_id, "purge"],
    )
    .map_err(ApiError::from_operation)?;
    let mut response = state
        .http_client
        .patch(purge_url.clone())
        .send()
        .await
        .map_err(ApiError::bad_gateway)?;

    if response.status() == StatusCode::CONFLICT {
        let kill_url = restate_url(
            &state.restate_admin_url,
            &["invocations", invocation_id, "kill"],
        )
        .map_err(ApiError::from_operation)?;
        let kill_response = state
            .http_client
            .patch(kill_url)
            .send()
            .await
            .map_err(ApiError::bad_gateway)?;
        if !kill_response.status().is_success() && kill_response.status() != StatusCode::NOT_FOUND {
            return Err(restate_admin_error("kill", kill_response).await);
        }

        response = state
            .http_client
            .patch(purge_url)
            .send()
            .await
            .map_err(ApiError::bad_gateway)?;
    }

    if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
        Ok(())
    } else {
        Err(restate_admin_error("purge", response).await)
    }
}

async fn restate_admin_error(operation: &str, response: reqwest::Response) -> ApiError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    tracing::warn!(%status, %body, "Restate admin {operation} failed");
    ApiError::new(
        StatusCode::BAD_GATEWAY,
        format!("Restate could not {operation} the workflow"),
    )
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn health(State(state): State<AppState>) -> Result<Json<HealthResponse>, ApiError> {
    sqlx::query("SELECT 1")
        .execute(state.review_store.pool())
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(HealthResponse { status: "ok" }))
}

#[derive(Serialize)]
struct UploadPdfResponse {
    workflow_id: String,
    pdf: StoredPdf,
    submission_status: u16,
}

async fn upload_pdf(
    State(state): State<AppState>,
    pdf: Bytes,
) -> Result<(StatusCode, Json<UploadPdfResponse>), ApiError> {
    validate_pdf_size(&pdf)?;
    let stored = state
        .garage_pipeline
        .execute(&sha256_hex(&pdf), &pdf.to_vec())
        .await
        .map_err(ApiError::storage)?
        .into_output(|_| {});
    let workflow_id = stored.pdf_hash.clone();
    state
        .garage_pipeline
        .metadata()
        .link_workflow(&workflow_id, &stored.pdf_hash)
        .await
        .map_err(ApiError::internal)?;
    let response = submit_pipeline(
        &state.http_client,
        &state.restate_ingress_url,
        &workflow_id,
        &stored.pdf_hash,
    )
    .await
    .map_err(ApiError::from_operation)?;
    let submission_status = response.status().as_u16();
    if !response.status().is_success() {
        return Err(restate_submission_error(response).await);
    }

    Ok((
        StatusCode::ACCEPTED,
        Json(UploadPdfResponse {
            workflow_id,
            pdf: stored,
            submission_status,
        }),
    ))
}

async fn download_pdf(
    State(state): State<AppState>,
    Path(pdf_hash): Path<String>,
) -> Result<Response, ApiError> {
    let Some((metadata, bytes)) = state
        .garage_pipeline
        .load(&pdf_hash)
        .await
        .map_err(ApiError::storage)?
    else {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "PDF not found"));
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, metadata.content_type)
        .header(header::CONTENT_LENGTH, bytes.len())
        .header("x-pdf-sha256", metadata.pdf_hash)
        .body(Body::from(bytes))
        .map_err(ApiError::internal)
}

fn validate_pdf_size(pdf: &[u8]) -> Result<(), ApiError> {
    if pdf.is_empty() || pdf.len() > MAX_PDF_BYTES {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "request body must contain a PDF no larger than 30 MiB",
        ));
    }
    if !pdf.starts_with(b"%PDF-") {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "request body is not a PDF file",
        ));
    }
    Ok(())
}

async fn restate_submission_error(response: reqwest::Response) -> ApiError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    tracing::warn!(%status, %body, "Restate pipeline submission failed");
    ApiError::new(
        StatusCode::BAD_GATEWAY,
        "Restate rejected pipeline submission",
    )
}

async fn submit_workflow(
    State(state): State<AppState>,
    Path(workflow_id): Path<String>,
    pdf: Bytes,
) -> Result<Response, ApiError> {
    validate_pdf_size(&pdf)?;

    let stored = state
        .garage_pipeline
        .execute(&workflow_id, &pdf.to_vec())
        .await
        .map_err(ApiError::storage)?
        .into_output(|_| {});
    state
        .garage_pipeline
        .metadata()
        .link_workflow(&workflow_id, &stored.pdf_hash)
        .await
        .map_err(ApiError::internal)?;

    let response = submit_pipeline(
        &state.http_client,
        &state.restate_ingress_url,
        &workflow_id,
        &stored.pdf_hash,
    )
    .await
    .map_err(ApiError::from_operation)?;

    proxy_response(response).await
}

async fn run_stored_pipeline_part(
    State(state): State<AppState>,
    Path((part, identifier)): Path<(PipelinePart, i64)>,
) -> Result<Json<scepa::operations::ArtifactOperationResponse>, ApiError> {
    let review_case = state
        .review_store
        .get_case(identifier)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "review artifact not found"))?;
    let pdf_hash = state
        .garage_pipeline
        .metadata()
        .hash_for_workflow(&review_case.workflow_id)
        .await
        .map_err(ApiError::internal)?;
    run_artifact_operation(
        &state.review_store,
        Some(&state.typedb),
        state.http_client,
        &state.grobid_url,
        part,
        identifier,
        pdf_hash.as_deref(),
    )
    .await
    .map(Json)
    .map_err(ApiError::from_operation)
}

async fn patch_and_run_stored_pipeline_part(
    State(state): State<AppState>,
    Path((part, identifier)): Path<(PipelinePart, i64)>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Json<scepa::operations::ArtifactOperationResponse>, ApiError> {
    if part == PipelinePart::Execute {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "execute is not a validation repair endpoint",
        ));
    }
    let review_case = state
        .review_store
        .get_case(identifier)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "review case not found"))?;
    let expected_phase = match part {
        PipelinePart::InputValidation => "input_validation",
        PipelinePart::OutputValidation => "output_validation",
        PipelinePart::Execute => unreachable!(),
    };
    if review_case.phase != expected_phase {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!(
                "requested {part} repair for a {} failure",
                review_case.phase
            ),
        ));
    }
    let _ = patch_review_artifact(State(state.clone()), Path(identifier), headers, bytes).await?;
    run_stored_pipeline_part(State(state), Path((part, identifier))).await
}

async fn run_pipeline(
    State(state): State<AppState>,
    Path(identifier): Path<String>,
    pdf: Bytes,
) -> Result<Response, ApiError> {
    submit_workflow(State(state), Path(identifier), pdf).await
}

#[derive(Serialize)]
struct BatchRunResponse {
    identifier: String,
    status: u16,
}

async fn run_pipeline_batch(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<Vec<BatchRunResponse>>, ApiError> {
    let mut results = Vec::new();
    while let Some(field) = multipart.next_field().await.map_err(ApiError::invalid)? {
        let filename = field
            .file_name()
            .ok_or_else(|| ApiError::invalid("each batch part must have a filename"))?
            .to_owned();
        let identifier = std::path::Path::new(&filename)
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ApiError::invalid("PDF filename must provide a valid identifier"))?
            .to_owned();
        let pdf = field.bytes().await.map_err(ApiError::invalid)?;
        if pdf.is_empty() || pdf.len() > MAX_PDF_BYTES {
            return Err(ApiError::invalid(format!(
                "{filename} must contain a PDF no larger than 30 MiB"
            )));
        }
        if !pdf.starts_with(b"%PDF-") {
            return Err(ApiError::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("{filename} is not a PDF file"),
            ));
        }
        let stored = state
            .garage_pipeline
            .execute(&identifier, &pdf.to_vec())
            .await
            .map_err(ApiError::storage)?
            .into_output(|_| {});
        state
            .garage_pipeline
            .metadata()
            .link_workflow(&identifier, &stored.pdf_hash)
            .await
            .map_err(ApiError::internal)?;
        let response = submit_pipeline(
            &state.http_client,
            &state.restate_ingress_url,
            &identifier,
            &stored.pdf_hash,
        )
        .await
        .map_err(ApiError::from_operation)?;
        results.push(BatchRunResponse {
            identifier,
            status: response.status().as_u16(),
        });
    }
    if results.is_empty() {
        return Err(ApiError::invalid("batch request contains no PDF files"));
    }
    Ok(Json(results))
}

async fn workflow_output(
    State(state): State<AppState>,
    Path(workflow_id): Path<String>,
) -> Result<Response, ApiError> {
    let url = restate_url(
        &state.restate_ingress_url,
        &[
            "restate",
            "workflow",
            "ScepaPipeline",
            &workflow_id,
            "output",
        ],
    )
    .map_err(ApiError::from_operation)?;
    let response = state
        .http_client
        .get(url)
        .send()
        .await
        .map_err(ApiError::bad_gateway)?;
    proxy_response(response).await
}

#[derive(Deserialize)]
struct ListCasesQuery {
    status: Option<String>,
    limit: Option<i64>,
}

async fn list_review_cases(
    State(state): State<AppState>,
    Query(query): Query<ListCasesQuery>,
) -> Result<Json<Vec<ReviewCase>>, ApiError> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let cases = state
        .review_store
        .list_cases(query.status.as_deref(), limit)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(cases))
}

#[derive(Serialize)]
struct ReviewQueueCountResponse {
    count: i64,
}

async fn count_pending_review_cases(
    State(state): State<AppState>,
) -> Result<Json<ReviewQueueCountResponse>, ApiError> {
    let count = state
        .review_store
        .count_pending_cases()
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(ReviewQueueCountResponse { count }))
}

async fn get_review_case(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<ReviewCase>, ApiError> {
    state
        .review_store
        .get_case(id)
        .await
        .map_err(ApiError::internal)?
        .map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "review case not found"))
}

async fn get_review_artifact(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let (content_type, bytes) = state
        .review_store
        .get_artifact(id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "review case not found"))?;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(bytes))
        .map_err(ApiError::internal)
}

#[derive(Serialize)]
struct PatchArtifactResponse {
    identifier: i64,
    content_type: String,
    size: usize,
}

async fn patch_review_artifact(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Json<PatchArtifactResponse>, ApiError> {
    let review_case = state
        .review_store
        .get_case(id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "review case not found"))?;
    if review_case.status != "pending" {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "only pending artifacts can be patched",
        ));
    }
    if review_case.phase == "processing" {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "only validation-failure artifacts can be patched",
        ));
    }
    if bytes.is_empty() || bytes.len() > MAX_PDF_BYTES {
        return Err(ApiError::invalid(
            "replacement artifact must be non-empty and no larger than 30 MiB",
        ));
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or(&review_case.artifact_content_type)
        .to_owned();
    let updated = state
        .review_store
        .patch_validation_artifact(id, &content_type, &bytes)
        .await
        .map_err(ApiError::internal)?;
    if !updated {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "artifact changed state before it could be patched",
        ));
    }
    Ok(Json(PatchArtifactResponse {
        identifier: id,
        content_type,
        size: bytes.len(),
    }))
}

#[derive(Deserialize)]
struct ResolveBody {
    decision: ReviewDecision,
}

async fn resolve_review_case(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<ResolveBody>,
) -> Result<Response, ApiError> {
    let review_case = state
        .review_store
        .get_case(id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "review case not found"))?;

    if review_case.status != "pending" {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "review case has already been resolved",
        ));
    }
    if matches!(body.decision, ReviewDecision::Retry) && !review_case.retryable {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "this failure was classified as terminal and cannot be retried",
        ));
    }

    let url = restate_url(
        &state.restate_ingress_url,
        &["ScepaPipeline", &review_case.workflow_id, "resolve_review"],
    )
    .map_err(ApiError::from_operation)?;
    let response = state
        .http_client
        .post(url)
        .json(&ResolveReviewRequest {
            review_case_id: id,
            decision: body.decision,
        })
        .send()
        .await
        .map_err(ApiError::bad_gateway)?;

    proxy_response(response).await
}

async fn proxy_response(response: reqwest::Response) -> Result<Response, ApiError> {
    let status = response.status();
    let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
    let bytes = response.bytes().await.map_err(ApiError::bad_gateway)?;
    let mut builder = Response::builder().status(status);
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    builder.body(Body::from(bytes)).map_err(ApiError::internal)
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(%error, "API operation failed");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
    }

    fn bad_gateway(error: impl std::fmt::Display) -> Self {
        tracing::warn!(%error, "Restate request failed");
        Self::new(StatusCode::BAD_GATEWAY, "Restate is unavailable")
    }

    fn storage(error: impl std::fmt::Display) -> Self {
        tracing::warn!(%error, "PDF storage operation failed");
        Self::new(StatusCode::BAD_GATEWAY, "PDF storage is unavailable")
    }

    fn invalid(error: impl std::fmt::Display) -> Self {
        Self::new(StatusCode::BAD_REQUEST, error.to_string())
    }

    fn from_operation(error: OperationError) -> Self {
        match error {
            OperationError::NotFound(message) => Self::new(StatusCode::NOT_FOUND, message),
            OperationError::Invalid(message) => {
                Self::new(StatusCode::UNPROCESSABLE_ENTITY, message)
            }
            OperationError::Internal(message) => {
                tracing::error!(%message, "pipeline operation failed");
                Self::new(StatusCode::BAD_GATEWAY, message)
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct ErrorBody {
            error: String,
        }

        (
            self.status,
            Json(ErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_and_deduplicates_invocation_ids_from_restate_query_json() {
        let value = serde_json::json!({
            "columns": ["id"],
            "rows": [
                {"id": "inv_first"},
                ["inv_second"],
                {"duplicate": "inv_first"},
                {"unrelated": "ScepaPipeline"}
            ]
        });
        let mut ids = Vec::new();
        collect_invocation_ids(&value, &mut ids);
        ids.sort_unstable();
        ids.dedup();

        assert_eq!(ids, ["inv_first", "inv_second"]);
    }
}
