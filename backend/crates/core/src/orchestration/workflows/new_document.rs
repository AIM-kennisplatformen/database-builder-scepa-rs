//! Durable ingestion workflow for newly uploaded documents.

use restate_sdk::prelude::{ContextClient, HandlerResult, Json, WorkflowContext};
use serde::{Deserialize, Serialize};

use super::super::restate::{
    DocumentRestateServiceClient, GarageRestateServiceClient, LinkWorkflowPdfRequest,
    PipelineExecuteRequest, PipelineExecuteResponse, TypeDbExecuteRequest,
    TypeDbRestateServiceClient,
};
use crate::{
    models::{canonical::CanonicalModel, draft::TeiDocument},
    pipeline::{DocumentPipelineWarning, garage::StoredPdf},
};

/// Input for a complete document-ingestion workflow.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewDocumentWorkflowRequest {
    pub pdf: Vec<u8>,
}

/// Result of a complete document-ingestion workflow.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewDocumentWorkflowResponse {
    pub stored_pdf: StoredPdf,
    pub document: TeiDocument,
    pub canonical: CanonicalModel,
    pub warnings: Vec<DocumentPipelineWarning>,
}

/// Input for the child workflow that extracts and parses one PDF.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentExtractionWorkflowRequest {
    pub workflow_id: String,
    pub pdf: Vec<u8>,
}

/// Parsed document and non-fatal findings produced by extraction.
pub type DocumentExtractionWorkflowResponse =
    PipelineExecuteResponse<TeiDocument, DocumentPipelineWarning>;

/// Extracts Grobid TEI and parses it as one child-workflow operation.
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

        let extracted = ctx
            .service_client::<DocumentRestateServiceClient>()
            .execute(Json(PipelineExecuteRequest::new(
                request.workflow_id,
                request.pdf,
            )))
            .call()
            .await?
            .into_inner();

        Ok(Json(PipelineExecuteResponse {
            output: extracted.output.into_document(),
            warnings: extracted.warnings,
        }))
    }
}

/// Uploads, indexes, extracts, and persists one document in strict order.
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

        // Garage uploads the object and upserts its immutable metadata before
        // its execute handler returns.
        let stored = ctx
            .service_client::<GarageRestateServiceClient>()
            .execute(Json(PipelineExecuteRequest::new(
                workflow_id.clone(),
                request.pdf.clone(),
            )))
            .call()
            .await?
            .into_inner()
            .output;

        // Keep this as a separate durable call so extraction cannot start
        // until the workflow-to-hash association exists in PostgreSQL.
        ctx.service_client::<GarageRestateServiceClient>()
            .link_workflow(Json(LinkWorkflowPdfRequest {
                workflow_id: workflow_id.clone(),
                pdf_hash: stored.pdf_hash.clone(),
            }))
            .call()
            .await?;

        let extracted = ctx
            .workflow_client::<DocumentExtractionWorkflowClient>(format!(
                "{workflow_id}:extraction"
            ))
            .run(Json(DocumentExtractionWorkflowRequest {
                workflow_id: workflow_id.clone(),
                pdf: request.pdf,
            }))
            .call()
            .await?
            .into_inner();

        let canonical = ctx
            .service_client::<TypeDbRestateServiceClient>()
            .execute(Json(TypeDbExecuteRequest {
                pdf_hash: stored.pdf_hash.clone(),
                document: extracted.output.clone(),
            }))
            .call()
            .await?
            .into_inner();

        Ok(Json(NewDocumentWorkflowResponse {
            stored_pdf: stored,
            document: extracted.output,
            canonical,
            warnings: extracted.warnings,
        }))
    }
}
