use restate_sdk::prelude::{Context, HandlerResult, Json, TerminalError};
use serde::{Deserialize, Serialize};

use crate::{
    models::draft::DraftDocument,
    postgres::{PostgresReviewStore, ReviewCase},
};

use super::to_postgres_handler_error;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoreArtifactRequest {
    pub pdf_hash: String,
    pub artifact: DraftDocument,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoreTeiRequest {
    pub pdf_hash: String,
    pub tei: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResolveReviewCaseRequest {
    pub case_id: i64,
    pub workflow_id: String,
    pub pdf_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepairDraft {
    pub case: ReviewCase,
    pub pdf_hash: String,
    pub draft: DraftDocument,
}

pub struct ArtifactRestateService {
    store: PostgresReviewStore,
}

impl ArtifactRestateService {
    pub fn new(store: PostgresReviewStore) -> Self {
        Self { store }
    }
}

#[restate_sdk::service(name = "ArtifactStore")]
impl ArtifactRestateService {
    #[restate_sdk::handler]
    async fn get_draft(
        &self,
        _ctx: Context<'_>,
        pdf_hash: Json<String>,
    ) -> HandlerResult<Json<Option<DraftDocument>>> {
        self.store
            .get_draft_document(&pdf_hash.into_inner())
            .await
            .map(Json)
            .map_err(to_postgres_handler_error)
    }

    #[restate_sdk::handler]
    async fn get_published(
        &self,
        _ctx: Context<'_>,
        pdf_hash: Json<String>,
    ) -> HandlerResult<Json<Option<DraftDocument>>> {
        self.store
            .get_published_document(&pdf_hash.into_inner())
            .await
            .map(|document| Json(document.map(|document| document.artifact)))
            .map_err(to_postgres_handler_error)
    }

    #[restate_sdk::handler]
    async fn get_repair(
        &self,
        _ctx: Context<'_>,
        case_id: Json<i64>,
    ) -> HandlerResult<Json<RepairDraft>> {
        let case = self
            .store
            .get_case(case_id.into_inner())
            .await
            .map_err(to_postgres_handler_error)?
            .filter(|case| case.status == "pending")
            .ok_or_else(|| TerminalError::new("pending review case not found"))?;
        let pdf_hash = case
            .pdf_hash
            .clone()
            .ok_or_else(|| TerminalError::new("review case is not linked to a source PDF"))?;
        let draft = self
            .store
            .get_repair_draft(&pdf_hash)
            .await
            .map_err(to_postgres_handler_error)?;
        Ok(Json(RepairDraft {
            case,
            pdf_hash,
            draft,
        }))
    }

    #[restate_sdk::handler]
    async fn store_tei(
        &self,
        _ctx: Context<'_>,
        request: Json<StoreTeiRequest>,
    ) -> HandlerResult<()> {
        let request = request.into_inner();
        self.store
            .store_tei_xml(&request.pdf_hash, &request.tei)
            .await
            .map_err(to_postgres_handler_error)
    }

    #[restate_sdk::handler]
    async fn store_draft(
        &self,
        _ctx: Context<'_>,
        request: Json<StoreArtifactRequest>,
    ) -> HandlerResult<()> {
        let request = request.into_inner();
        self.store
            .store_draft_artifact(&request.pdf_hash, &request.artifact)
            .await
            .map_err(to_postgres_handler_error)
    }

    #[restate_sdk::handler]
    async fn store_published(
        &self,
        _ctx: Context<'_>,
        request: Json<StoreArtifactRequest>,
    ) -> HandlerResult<()> {
        let request = request.into_inner();
        let stored = self
            .store
            .store_published_artifact(&request.pdf_hash, &request.artifact)
            .await
            .map_err(to_postgres_handler_error)?;
        if !stored {
            return Err(TerminalError::new("document artifact was not found").into());
        }
        Ok(())
    }

    #[restate_sdk::handler]
    async fn resolve_review_case(
        &self,
        _ctx: Context<'_>,
        request: Json<ResolveReviewCaseRequest>,
    ) -> HandlerResult<()> {
        let request = request.into_inner();
        let resolved = self
            .store
            .resolve_case(
                request.case_id,
                &request.workflow_id,
                "resolved",
                serde_json::json!({
                    "action": "manually_fixed",
                    "pdf_hash": request.pdf_hash,
                    "enriched": false,
                }),
            )
            .await
            .map_err(to_postgres_handler_error)?;
        if !resolved {
            let already_resolved = self
                .store
                .get_case(request.case_id)
                .await
                .map_err(to_postgres_handler_error)?
                .is_some_and(|case| {
                    case.workflow_id == request.workflow_id && case.status == "resolved"
                });
            if !already_resolved {
                return Err(TerminalError::new("review case was already resolved").into());
            }
        }
        Ok(())
    }
}
