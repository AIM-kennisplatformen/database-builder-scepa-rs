use restate_sdk::prelude::{ContextClient, HandlerResult, Json, WorkflowContext};
use serde::{Deserialize, Serialize};

use crate::{
    models::draft::{DraftDocument, TeiDocument},
    pipeline::DocumentPipelineWarning,
    restate::services::{
        ArtifactRestateServiceClient, GrobidRestateServiceClient, PipelineExecuteRequest,
        PipelineExecuteResponse, StoreArtifactRequest, StoreTeiRequest, TeiRestateServiceClient,
    },
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentExtractionWorkflowRequest {
    pub workflow_id: String,
    pub pdf_hash: String,
}

pub type DocumentExtractionWorkflowResponse =
    PipelineExecuteResponse<TeiDocument, DocumentPipelineWarning>;

pub struct DocumentExtractionWorkflow;

#[restate_sdk::workflow(name = "DocumentExtractionWorkflow")]
impl DocumentExtractionWorkflow {
    #[restate_sdk::handler]
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        request: Json<DocumentExtractionWorkflowRequest>,
    ) -> HandlerResult<Json<DocumentExtractionWorkflowResponse>> {
        let request = request.into_inner();
        let grobid = ctx
            .service_client::<GrobidRestateServiceClient>()
            .execute(Json(PipelineExecuteRequest::new(
                request.workflow_id.clone(),
                request.pdf_hash.clone(),
            )))
            .call()
            .await?
            .into_inner();

        ctx.service_client::<ArtifactRestateServiceClient>()
            .store_tei(Json(StoreTeiRequest {
                pdf_hash: request.pdf_hash.clone(),
                tei: grobid.output.clone(),
            }))
            .call()
            .await?;

        let converted = ctx
            .service_client::<TeiRestateServiceClient>()
            .execute(Json(PipelineExecuteRequest::new(
                request.workflow_id,
                grobid.output,
            )))
            .call()
            .await?
            .into_inner();
        let draft = DraftDocument::new(converted.output.clone());
        ctx.service_client::<ArtifactRestateServiceClient>()
            .store_draft(Json(StoreArtifactRequest {
                pdf_hash: request.pdf_hash,
                artifact: draft,
            }))
            .call()
            .await?;

        Ok(Json(PipelineExecuteResponse {
            output: converted.output,
            warnings: grobid
                .warnings
                .into_iter()
                .map(DocumentPipelineWarning::Grobid)
                .chain(
                    converted
                        .warnings
                        .into_iter()
                        .map(DocumentPipelineWarning::Tei),
                )
                .collect(),
        }))
    }
}
