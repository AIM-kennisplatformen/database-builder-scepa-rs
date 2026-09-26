//! Transport-independent entry workflow for PDF uploads.

use crate::pipeline::{PipelineService, garage::GaragePipelineService};
use crate::restate::{RestateClient, RestateError, workflows::NewDocumentWorkflowResponse};

#[derive(Clone)]
pub struct DocumentUpload {
    pdfs: GaragePipelineService,
    restate: RestateClient,
}

#[derive(Clone, Debug)]
pub struct ReviewedUpload {
    pub workflow_id: String,
    pub result: NewDocumentWorkflowResponse,
}

#[derive(Debug)]
pub struct FailedUpload {
    pub workflow_id: String,
    pub source: DocumentUploadError,
}

#[derive(Debug, thiserror::Error)]
pub enum DocumentUploadError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("document storage failed: {0}")]
    Storage(#[source] std::io::Error),
    #[error(transparent)]
    Workflow(#[from] RestateError),
}

impl DocumentUpload {
    pub fn new(pdfs: GaragePipelineService, restate: RestateClient) -> Self {
        Self { pdfs, restate }
    }

    /// Stores a PDF and runs the complete new-document workflow.
    pub async fn run(&self, pdf: Vec<u8>) -> Result<ReviewedUpload, FailedUpload> {
        let workflow_id = format!("upload:{}", uuid::Uuid::new_v4());
        let stored = self
            .store(&workflow_id, &pdf)
            .await
            .map_err(|source| FailedUpload {
                workflow_id: workflow_id.clone(),
                source: DocumentUploadError::Storage(source),
            })?;
        let result = self
            .restate
            .run_new_document(&workflow_id, stored.pdf_hash)
            .await
            .map_err(|source| FailedUpload {
                workflow_id: workflow_id.clone(),
                source: DocumentUploadError::Workflow(source),
            })?;
        Ok(ReviewedUpload {
            workflow_id,
            result,
        })
    }

    /// Stores and durably submits a PDF for automatic canonical publication.
    pub async fn submit(&self, workflow_id: &str, pdf: Vec<u8>) -> Result<(), DocumentUploadError> {
        if workflow_id.is_empty() {
            return Err(DocumentUploadError::InvalidInput(
                "workflow identifier must not be empty".into(),
            ));
        }
        let stored = self
            .store(workflow_id, &pdf)
            .await
            .map_err(DocumentUploadError::Storage)?;
        self.restate
            .submit_new_document(workflow_id, stored.pdf_hash)
            .await
            .map_err(DocumentUploadError::Workflow)?;
        Ok(())
    }

    async fn store(
        &self,
        workflow_id: &str,
        pdf: &[u8],
    ) -> std::io::Result<crate::pipeline::garage::StoredPdf> {
        self.pdfs
            .execute(workflow_id, &pdf.to_vec())
            .await
            .map(|outcome| outcome.into_output(|_| {}))
            .map_err(crate::conflict::pipeline_io_error)
    }
}
