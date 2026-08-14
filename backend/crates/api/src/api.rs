//! Public Axum API for uploading new documents.

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use scepa::{
    models::{
        canonical::CanonicalModel,
        draft::{DraftDocument, ManualDocument},
    },
    orchestration::{
        NewDocumentIngressClient, NewDocumentWorkflowResponse, UpdateDocumentIngressClient,
        UpdateDocumentWorkflowRequest, UpdateDocumentWorkflowResponse,
    },
    pipeline::{
        PipelineService,
        garage::{GaragePipelineService, StoredPdf, sha256_hex},
        typedb::{TypeDbService, TypeDbStore},
    },
    postgres::{PostgresReviewStore, PublishedDocument, PublishedDocumentSummary},
};
use serde::Serialize;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

const MAX_PDF_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    workflows: NewDocumentIngressClient,
    updates: UpdateDocumentIngressClient,
    pdfs: GaragePipelineService,
    drafts: PostgresReviewStore,
    typedb: TypeDbService<TypeDbStore>,
}

impl AppState {
    pub fn new(
        workflows: NewDocumentIngressClient,
        updates: UpdateDocumentIngressClient,
        pdfs: GaragePipelineService,
        drafts: PostgresReviewStore,
        typedb: TypeDbService<TypeDbStore>,
    ) -> Self {
        Self {
            workflows,
            updates,
            pdfs,
            drafts,
            typedb,
        }
    }
}

#[derive(Serialize)]
struct UploadResponse {
    workflow_id: String,
    result: NewDocumentWorkflowResponse,
}

#[derive(Serialize)]
struct SubmissionResponse {
    workflow_id: String,
}

#[derive(Serialize)]
struct DraftResponse {
    pdf_hash: String,
    #[serde(flatten)]
    draft: DraftDocument,
}

#[derive(Serialize)]
struct PublishResponse {
    artifact: DraftResponse,
    canonical: CanonicalModel,
}

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, self.1).into_response()
    }
}

/// Builds the document-upload HTTP router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/pdfs", post(upload_pdf))
        .route("/pdfs/{pdf_hash}", get(download_pdf))
        .route("/pdfs/submissions/{workflow_id}", post(submit_pdf))
        .route("/drafts/{pdf_hash}", get(get_draft).put(publish_draft))
        .route("/documents", get(list_documents))
        .route(
            "/documents/requiring-fixing",
            get(list_documents_requiring_fixing),
        )
        .route(
            "/documents/{pdf_hash}",
            get(get_published_document).put(update_document),
        )
        .layer(DefaultBodyLimit::max(MAX_PDF_BYTES))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn download_pdf(
    State(state): State<AppState>,
    Path(pdf_hash): Path<String>,
) -> Result<Response, ApiError> {
    let (metadata, pdf) = state
        .pdfs
        .load(&pdf_hash)
        .await
        .map_err(internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "PDF not found".into()))?;
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
        pdf,
    )
        .into_response())
}

async fn upload_pdf(
    State(state): State<AppState>,
    pdf: Bytes,
) -> Result<(StatusCode, Json<UploadResponse>), ApiError> {
    let workflow_id = sha256_hex(&pdf);
    let stored = store_pdf(&state, &workflow_id, pdf).await?;
    let result = state
        .workflows
        .run(&workflow_id, stored.pdf_hash)
        .await
        .map_err(bad_gateway)?;

    Ok((
        StatusCode::CREATED,
        Json(UploadResponse {
            workflow_id,
            result,
        }),
    ))
}

async fn submit_pdf(
    State(state): State<AppState>,
    Path(workflow_id): Path<String>,
    pdf: Bytes,
) -> Result<(StatusCode, Json<SubmissionResponse>), ApiError> {
    if workflow_id.is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "workflow identifier must not be empty".into(),
        ));
    }

    let stored = store_pdf(&state, &workflow_id, pdf).await?;
    state
        .workflows
        .submit(&workflow_id, stored.pdf_hash)
        .await
        .map_err(bad_gateway)?;

    Ok((
        StatusCode::ACCEPTED,
        Json(SubmissionResponse { workflow_id }),
    ))
}

async fn store_pdf(state: &AppState, workflow_id: &str, pdf: Bytes) -> Result<StoredPdf, ApiError> {
    state
        .pdfs
        .execute(workflow_id, &pdf.to_vec())
        .await
        .map(|outcome| outcome.into_output(|_| {}))
        .map_err(bad_gateway)
}

async fn get_draft(
    State(state): State<AppState>,
    Path(pdf_hash): Path<String>,
) -> Result<Json<DraftResponse>, ApiError> {
    let draft = state
        .drafts
        .get_draft_document(&pdf_hash)
        .await
        .map_err(internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "document draft not found".into()))?;

    Ok(Json(DraftResponse { pdf_hash, draft }))
}

async fn publish_draft(
    State(state): State<AppState>,
    Path(pdf_hash): Path<String>,
    Json(manual_data): Json<ManualDocument>,
) -> Result<Json<PublishResponse>, ApiError> {
    if !state
        .drafts
        .store_manual_data(&pdf_hash, &manual_data)
        .await
        .map_err(internal)?
    {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "document draft not found".into(),
        ));
    }

    let draft = state
        .drafts
        .get_draft_document(&pdf_hash)
        .await
        .map_err(internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "document draft not found".into()))?;
    let effective = draft.effective_document();
    let canonical = state
        .typedb
        .pre_validate_with_pdf_hash(&effective, &pdf_hash)
        .await
        .map_err(|error| ApiError(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?;
    state.typedb.execute(&canonical).await.map_err(internal)?;
    if !state
        .drafts
        .store_published_artifact(&pdf_hash, &draft)
        .await
        .map_err(internal)?
    {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "document draft not found".into(),
        ));
    }

    Ok(Json(PublishResponse {
        artifact: DraftResponse { pdf_hash, draft },
        canonical,
    }))
}

async fn list_documents(
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

async fn list_documents_requiring_fixing(
    State(state): State<AppState>,
) -> Result<Json<Vec<scepa::postgres::ReviewCase>>, ApiError> {
    Ok(Json(
        state
            .drafts
            .list_documents_requiring_fixing()
            .await
            .map_err(internal)?,
    ))
}

async fn get_published_document(
    State(state): State<AppState>,
    Path(pdf_hash): Path<String>,
) -> Result<Json<PublishedDocument>, ApiError> {
    state
        .drafts
        .get_published_document(&pdf_hash)
        .await
        .map_err(internal)?
        .map(Json)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "published document not found".into()))
}

async fn update_document(
    State(state): State<AppState>,
    Path(pdf_hash): Path<String>,
    Json(manual_data): Json<ManualDocument>,
) -> Result<Json<UpdateDocumentWorkflowResponse>, ApiError> {
    let revision = sha256_hex(&serde_json::to_vec(&manual_data).map_err(internal)?);
    let workflow_id = format!("{pdf_hash}:update:{revision}");
    let result = state
        .updates
        .run(
            &workflow_id,
            UpdateDocumentWorkflowRequest {
                pdf_hash,
                manual_data,
            },
        )
        .await
        .map_err(bad_gateway)?;
    Ok(Json(result))
}

fn bad_gateway(error: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_GATEWAY, error.to_string())
}

fn internal(error: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}
