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
    document_upload::DocumentUpload,
    models::{
        canonical::CanonicalModel,
        draft::{DraftDocument, ManualDocument},
    },
    pipeline::garage::GaragePipelineService,
    postgres::{PostgresReviewStore, PublishedDocument, PublishedDocumentSummary},
    restate::{
        RestateClient,
        services::RepairDraft,
        workflows::{
            FixDocumentWorkflowRequest, NewDocumentWorkflowResponse, UpdateDocumentWorkflowResponse,
        },
    },
};
use serde::{Deserialize, Serialize};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

const MAX_PDF_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    uploads: DocumentUpload,
    restate: RestateClient,
    pdfs: GaragePipelineService,
    drafts: PostgresReviewStore,
}

impl AppState {
    pub fn new(
        restate: RestateClient,
        pdfs: GaragePipelineService,
        drafts: PostgresReviewStore,
    ) -> Self {
        Self {
            uploads: DocumentUpload::new(pdfs.clone(), restate.clone()),
            restate,
            pdfs,
            drafts,
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

#[derive(Serialize)]
struct RepairDraftResponse {
    case: scepa::postgres::ReviewCase,
    draft: DraftResponse,
}

#[derive(Deserialize)]
struct RepairDocumentRequest {
    manual_data: ManualDocument,
    #[serde(default)]
    enrich: bool,
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
            "/documents/requiring-fixing/{case_id}",
            get(get_document_requiring_fixing).put(fix_document),
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
    let (metadata, bytes) = state
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
        bytes,
    )
        .into_response())
}

async fn upload_pdf(
    State(state): State<AppState>,
    pdf: Bytes,
) -> Result<(StatusCode, Json<UploadResponse>), ApiError> {
    let upload = state.uploads.run(pdf.to_vec()).await.map_err(upstream)?;

    Ok((
        StatusCode::CREATED,
        Json(UploadResponse {
            workflow_id: upload.workflow_id,
            result: upload.result,
        }),
    ))
}

async fn submit_pdf(
    State(state): State<AppState>,
    Path(workflow_id): Path<String>,
    pdf: Bytes,
) -> Result<(StatusCode, Json<SubmissionResponse>), ApiError> {
    state
        .uploads
        .submit(&workflow_id, pdf.to_vec())
        .await
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::InvalidInput {
                ApiError(StatusCode::BAD_REQUEST, error.to_string())
            } else {
                upstream(error)
            }
        })?;

    Ok((
        StatusCode::ACCEPTED,
        Json(SubmissionResponse { workflow_id }),
    ))
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
    let published = state
        .restate
        .publish_draft(pdf_hash.clone(), manual_data)
        .await
        .map_err(upstream)?;

    Ok(Json(PublishResponse {
        artifact: DraftResponse {
            pdf_hash,
            draft: published.artifact,
        },
        canonical: published.canonical,
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

async fn get_document_requiring_fixing(
    State(state): State<AppState>,
    Path(case_id): Path<i64>,
) -> Result<Json<RepairDraftResponse>, ApiError> {
    let repair = load_repair(&state.drafts, case_id).await?;

    Ok(Json(RepairDraftResponse {
        case: repair.case,
        draft: DraftResponse {
            pdf_hash: repair.pdf_hash,
            draft: repair.draft,
        },
    }))
}

async fn fix_document(
    State(state): State<AppState>,
    Path(case_id): Path<i64>,
    Json(request): Json<RepairDocumentRequest>,
) -> Result<Json<PublishResponse>, ApiError> {
    let result = state
        .restate
        .fix_document(FixDocumentWorkflowRequest {
            case_id,
            manual_data: request.manual_data,
            enrich: request.enrich,
        })
        .await
        .map_err(upstream)?;

    Ok(Json(PublishResponse {
        artifact: DraftResponse {
            pdf_hash: result.pdf_hash,
            draft: result.artifact,
        },
        canonical: result.canonical,
    }))
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
    let result = state
        .restate
        .update_document(pdf_hash, manual_data)
        .await
        .map_err(upstream)?;
    Ok(Json(result))
}

async fn load_repair(drafts: &PostgresReviewStore, case_id: i64) -> Result<RepairDraft, ApiError> {
    let case = drafts
        .get_case(case_id)
        .await
        .map_err(internal)?
        .filter(|case| case.status == "pending")
        .ok_or_else(|| {
            ApiError(
                StatusCode::NOT_FOUND,
                "pending review case not found".into(),
            )
        })?;
    let pdf_hash = case.pdf_hash.clone().ok_or_else(|| {
        ApiError(
            StatusCode::CONFLICT,
            "review case is not linked to a source PDF".into(),
        )
    })?;
    let draft = drafts.get_repair_draft(&pdf_hash).await.map_err(internal)?;
    Ok(RepairDraft {
        case,
        pdf_hash,
        draft,
    })
}

fn internal(error: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn upstream(error: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_GATEWAY, error.to_string())
}
