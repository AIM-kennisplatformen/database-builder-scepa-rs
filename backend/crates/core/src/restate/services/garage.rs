use restate_sdk::prelude::{Context, HandlerResult, Json, TerminalError};
use serde::{Deserialize, Serialize};

use crate::pipeline::garage::{GaragePipelineService, StoredPdf};

use super::to_postgres_handler_error;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinkWorkflowPdfRequest {
    pub workflow_id: String,
    pub pdf_hash: String,
}

pub struct GarageRestateService {
    pipeline: GaragePipelineService,
}

impl GarageRestateService {
    pub fn new(pipeline: GaragePipelineService) -> Self {
        Self { pipeline }
    }
}

#[restate_sdk::service(name = "GaragePipeline")]
impl GarageRestateService {
    #[restate_sdk::handler]
    async fn link_workflow(
        &self,
        _ctx: Context<'_>,
        request: Json<LinkWorkflowPdfRequest>,
    ) -> HandlerResult<()> {
        let request = request.into_inner();
        self.pipeline
            .metadata()
            .link_workflow(&request.workflow_id, &request.pdf_hash)
            .await
            .map_err(to_postgres_handler_error)
    }

    #[restate_sdk::handler]
    async fn get_pdf(
        &self,
        _ctx: Context<'_>,
        pdf_hash: Json<String>,
    ) -> HandlerResult<Json<StoredPdf>> {
        self.pipeline
            .metadata()
            .get(&pdf_hash.into_inner())
            .await
            .map_err(to_postgres_handler_error)?
            .map(Json)
            .ok_or_else(|| TerminalError::new("PDF metadata was not found").into())
    }
}
