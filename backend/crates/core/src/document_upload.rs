//! Transport-independent entry workflow for PDF uploads.

use crate::pipeline::{
    PipelineService,
    garage::{GaragePipelineService, sha256_hex},
};
use crate::restate::{RestateClient, workflows::NewDocumentWorkflowResponse};

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

impl DocumentUpload {
    pub fn new(pdfs: GaragePipelineService, restate: RestateClient) -> Self {
        Self { pdfs, restate }
    }

    /// Stores a PDF and runs the complete new-document workflow.
    pub async fn run(&self, pdf: Vec<u8>) -> std::io::Result<ReviewedUpload> {
        let workflow_id = sha256_hex(&pdf);
        let stored = self.store(&workflow_id, &pdf).await?;
        let result = self
            .restate
            .run_new_document(&workflow_id, stored.pdf_hash)
            .await?;
        Ok(ReviewedUpload {
            workflow_id,
            result,
        })
    }

    /// Stores and durably submits a PDF for automatic canonical publication.
    pub async fn submit(&self, workflow_id: &str, pdf: Vec<u8>) -> std::io::Result<()> {
        if workflow_id.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "workflow identifier must not be empty",
            ));
        }
        let stored = self.store(workflow_id, &pdf).await?;
        self.restate
            .submit_new_document(workflow_id, stored.pdf_hash)
            .await?;
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
            .map_err(|error| std::io::Error::other(error.to_string()))
    }
}
