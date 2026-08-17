use restate_sdk::prelude::{Context, HandlerError, HandlerResult, Json, TerminalError};

use crate::{
    pipeline::{
        garage::GaragePipelineService,
        grobid::{GrobidExtractionService, GrobidValidationWarning, HttpGrobidClient},
    },
    postgres::PostgresReviewStore,
};

use super::{PipelineExecuteRequest, PipelineExecuteResponse, execute_pipeline};

pub struct GrobidRestateService {
    pipeline: GrobidExtractionService<HttpGrobidClient, PostgresReviewStore>,
    pdfs: GaragePipelineService,
}

impl GrobidRestateService {
    pub fn new(
        pipeline: GrobidExtractionService<HttpGrobidClient, PostgresReviewStore>,
        pdfs: GaragePipelineService,
    ) -> Self {
        Self { pipeline, pdfs }
    }
}

#[restate_sdk::service(name = "GrobidExtractionPipeline")]
impl GrobidRestateService {
    #[restate_sdk::handler]
    async fn execute(
        &self,
        _ctx: Context<'_>,
        request: Json<PipelineExecuteRequest<String>>,
    ) -> HandlerResult<Json<PipelineExecuteResponse<String, GrobidValidationWarning>>> {
        let request = request.into_inner();
        let (_, pdf) = self
            .pdfs
            .load(&request.input)
            .await
            .map_err(|error| HandlerError::from(std::io::Error::other(error.to_string())))?
            .ok_or_else(|| TerminalError::new(format!("PDF {} was not found", request.input)))?;
        execute_pipeline(
            &self.pipeline,
            PipelineExecuteRequest::new(request.workflow_id, pdf),
        )
        .await
        .map(PipelineExecuteResponse::from)
        .map(Json)
    }
}
