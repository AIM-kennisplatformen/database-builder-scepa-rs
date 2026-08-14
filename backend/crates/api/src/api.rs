//! Public Axum API for uploading new documents.

use axum::{Router, body::Bytes, extract::DefaultBodyLimit, http::StatusCode, routing::post};
use tower_http::trace::TraceLayer;

const MAX_PDF_BYTES: usize = 30 * 1024 * 1024;

/// Builds the document-upload HTTP router.
pub fn router() -> Router {
    Router::new()
        .route("/pdfs", post(upload_pdf))
        .layer(DefaultBodyLimit::max(MAX_PDF_BYTES))
        .layer(TraceLayer::new_for_http())
}

async fn upload_pdf(_pdf: Bytes) -> StatusCode {
    todo!()
}
