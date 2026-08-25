use restate_sdk::prelude::{ContextClient, HandlerResult, Json, WorkflowContext};
use serde::{Deserialize, Serialize};

use crate::{
    models::{canonical::CanonicalModel, draft::DraftDocument},
    pipeline::{DocumentPipelineWarning, garage::StoredPdf},
    restate::{
        services::{
            ArtifactRestateServiceClient, GarageRestateServiceClient, LinkWorkflowPdfRequest,
            StoreArtifactRequest, TypeDbExecuteRequest, TypeDbRestateServiceClient,
        },
        workflows::{DocumentExtractionWorkflowClient, DocumentExtractionWorkflowRequest},
    },
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewDocumentWorkflowRequest {
    pub pdf_hash: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct NewDocumentWorkflowResponse {
    pub stored_pdf: StoredPdf,
    pub draft: DraftDocument,
    pub canonical: CanonicalModel,
    pub warnings: Vec<DocumentPipelineWarning>,
}

pub struct NewDocumentWorkflow;

#[restate_sdk::workflow(name = "NewDocumentWorkflow")]
impl NewDocumentWorkflow {
    #[restate_sdk::handler]
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        request: Json<NewDocumentWorkflowRequest>,
    ) -> HandlerResult<Json<NewDocumentWorkflowResponse>> {
        let workflow_id = ctx.key().to_owned();
        let request = request.into_inner();
        let stored = ctx
            .service_client::<GarageRestateServiceClient>()
            .get_pdf(Json(request.pdf_hash.clone()))
            .call()
            .await?
            .into_inner();
        ctx.service_client::<GarageRestateServiceClient>()
            .link_workflow(Json(LinkWorkflowPdfRequest {
                workflow_id: workflow_id.clone(),
                pdf_hash: request.pdf_hash.clone(),
            }))
            .call()
            .await?;
        let extracted = ctx
            .workflow_client::<DocumentExtractionWorkflowClient>(format!(
                "{workflow_id}:extraction"
            ))
            .run(Json(DocumentExtractionWorkflowRequest {
                workflow_id: workflow_id.clone(),
                pdf_hash: request.pdf_hash,
            }))
            .call()
            .await?
            .into_inner();
        let draft = DraftDocument::new(extracted.output);
        let canonical = ctx
            .service_client::<TypeDbRestateServiceClient>()
            .execute(Json(TypeDbExecuteRequest {
                workflow_id,
                pdf_hash: stored.pdf_hash.clone(),
                document: draft.effective_document(),
            }))
            .call()
            .await?
            .into_inner();
        ctx.service_client::<ArtifactRestateServiceClient>()
            .store_published(Json(StoreArtifactRequest {
                pdf_hash: stored.pdf_hash.clone(),
                artifact: draft.clone(),
            }))
            .call()
            .await?;
        Ok(Json(NewDocumentWorkflowResponse {
            stored_pdf: stored,
            draft,
            canonical,
            warnings: extracted.warnings,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_request_contains_only_the_pdf_reference() {
        let hash = "a".repeat(64);
        assert_eq!(
            serde_json::to_value(NewDocumentWorkflowRequest {
                pdf_hash: hash.clone()
            })
            .unwrap(),
            serde_json::json!({ "pdf_hash": hash })
        );
    }
}
