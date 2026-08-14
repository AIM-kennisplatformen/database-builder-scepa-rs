//! Durable workflow for selectively updating an already published document.

use restate_sdk::prelude::{ContextClient, HandlerResult, Json, TerminalError, WorkflowContext};
use serde::{Deserialize, Serialize};

use super::super::restate::{
    PublishedArtifactRestateServiceClient, StorePublishedArtifactRequest,
    TypeDbRestateServiceClient, TypeDbUpdateRequest,
};
use crate::{
    models::{
        canonical::CanonicalModel,
        draft::{DraftDocument, ManualDocument},
    },
    pipeline::typedb::CanonicalUpdateSummary,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UpdateDocumentWorkflowRequest {
    pub pdf_hash: String,
    pub manual_data: ManualDocument,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UpdateDocumentWorkflowResponse {
    pub artifact: DraftDocument,
    pub canonical: CanonicalModel,
    pub changes: CanonicalUpdateSummary,
}

pub struct UpdateDocumentWorkflow;

#[restate_sdk::workflow(name = "UpdateDocumentWorkflow")]
impl UpdateDocumentWorkflow {
    #[restate_sdk::handler]
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        request: Json<UpdateDocumentWorkflowRequest>,
    ) -> HandlerResult<Json<UpdateDocumentWorkflowResponse>> {
        let request = request.into_inner();
        let old_artifact = ctx
            .service_client::<PublishedArtifactRestateServiceClient>()
            .get_published(Json(request.pdf_hash.clone()))
            .call()
            .await?
            .into_inner()
            .ok_or_else(|| TerminalError::new("published document was not found"))?;

        let mut new_artifact = old_artifact.clone();
        new_artifact.manual_data = request.manual_data;
        let updated = ctx
            .service_client::<TypeDbRestateServiceClient>()
            .update(Json(TypeDbUpdateRequest {
                pdf_hash: request.pdf_hash.clone(),
                old_document: old_artifact.effective_document(),
                new_document: new_artifact.effective_document(),
            }))
            .call()
            .await?
            .into_inner();

        ctx.service_client::<PublishedArtifactRestateServiceClient>()
            .store_published(Json(StorePublishedArtifactRequest {
                pdf_hash: request.pdf_hash,
                artifact: new_artifact.clone(),
            }))
            .call()
            .await?;

        Ok(Json(UpdateDocumentWorkflowResponse {
            artifact: new_artifact,
            canonical: updated.canonical,
            changes: updated.changes,
        }))
    }
}
