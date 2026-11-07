use super::super::*;
use axum::extract::Query;
use scepa::postgres::UpdateDocumentPage;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub(crate) struct UpdateDocumentQuery {
    #[serde(default)]
    search: String,
    #[serde(default = "default_status")]
    status: String,
    #[serde(default = "default_sort")]
    sort_by: String,
    #[serde(default)]
    descending: bool,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_page_size")]
    page_size: i64,
}

fn default_status() -> String {
    "all".into()
}
fn default_sort() -> String {
    "title".into()
}
fn default_page() -> i64 {
    1
}
fn default_page_size() -> i64 {
    7
}

#[utoipa::path(
    get,
    path = "/documents/update-list",
    params(UpdateDocumentQuery),
    responses(
        (status = 200, description = "Filtered update-document page", body = UpdateDocumentPage),
        (status = 400, description = "Invalid list parameters", body = ErrorResponse),
        (status = 500, description = "Persistence error", body = ErrorResponse)
    ),
    tag = "documents"
)]
pub(crate) async fn list_update_documents(
    State(state): State<AppState>,
    Query(query): Query<UpdateDocumentQuery>,
) -> Result<Json<UpdateDocumentPage>, ApiError> {
    if !matches!(
        query.status.as_str(),
        "all" | "published" | "requiresFixing"
    ) || !matches!(query.sort_by.as_str(), "title" | "published_at")
        || query.page < 1
        || !(1..=100).contains(&query.page_size)
        || (query.page - 1).checked_mul(query.page_size).is_none()
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "Invalid list parameters",
            "Check status, sort_by, page, and page_size",
        ));
    }
    Ok(Json(
        state
            .drafts
            .list_update_documents(
                &query.search,
                &query.status,
                &query.sort_by,
                !query.descending,
                query.page,
                query.page_size,
            )
            .await
            .map_err(internal)?,
    ))
}
