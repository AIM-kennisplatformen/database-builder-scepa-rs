use restate_sdk::prelude::{ContextClient, HandlerResult, Json, TerminalError, WorkflowContext};
use serde::{Deserialize, Serialize};

use crate::{
    models::{
        canonical::CanonicalModel,
        draft::{DraftDocument, ManualDocument},
    },
    restate::{
        services::ArtifactRestateServiceClient,
        workflows::{
            ReviewCaseReference, UpdateDocumentWorkflowClient, UpdateDocumentWorkflowRequest,
        },
    },
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FixDocumentWorkflowRequest {
    pub case_id: i64,
    pub manual_data: ManualDocument,
    #[serde(default)]
    pub enrich: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FixDocumentWorkflowResponse {
    pub pdf_hash: String,
    pub artifact: DraftDocument,
    pub canonical: CanonicalModel,
}

pub struct FixDocumentWorkflow;

#[restate_sdk::workflow(name = "FixDocumentWorkflow")]
impl FixDocumentWorkflow {
    #[restate_sdk::handler]
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        request: Json<FixDocumentWorkflowRequest>,
    ) -> HandlerResult<Json<FixDocumentWorkflowResponse>> {
        let request = request.into_inner();
        if request.enrich {
            return Err(TerminalError::new("external enrichment is not available yet").into());
        }
        let repair = ctx
            .service_client::<ArtifactRestateServiceClient>()
            .get_repair(Json(request.case_id))
            .call()
            .await?
            .into_inner();
        let updated = ctx
            .workflow_client::<UpdateDocumentWorkflowClient>(format!("{}:update", ctx.key()))
            .run(Json(UpdateDocumentWorkflowRequest {
                pdf_hash: repair.pdf_hash.clone(),
                manual_data: request.manual_data,
                review_case: Some(ReviewCaseReference {
                    id: repair.case.id,
                    workflow_id: repair.case.workflow_id,
                }),
            }))
            .call()
            .await?
            .into_inner();
        Ok(Json(FixDocumentWorkflowResponse {
            pdf_hash: repair.pdf_hash,
            artifact: updated.artifact,
            canonical: updated.canonical,
        }))
    }
}
