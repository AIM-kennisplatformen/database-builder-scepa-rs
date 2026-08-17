use restate_sdk::prelude::{Context, HandlerResult, Json};

use crate::{
    models::draft::TeiDocument,
    pipeline::tei::{TeiConversionService, TeiValidationWarning},
    postgres::PostgresReviewStore,
};

use super::{PipelineExecuteRequest, PipelineExecuteResponse, execute_pipeline};

pub struct TeiRestateService {
    pipeline: TeiConversionService<PostgresReviewStore>,
}

impl TeiRestateService {
    pub fn new(pipeline: TeiConversionService<PostgresReviewStore>) -> Self {
        Self { pipeline }
    }
}

#[restate_sdk::service(name = "TeiConversionPipeline")]
impl TeiRestateService {
    #[restate_sdk::handler]
    async fn execute(
        &self,
        _ctx: Context<'_>,
        request: Json<PipelineExecuteRequest<String>>,
    ) -> HandlerResult<Json<PipelineExecuteResponse<TeiDocument, TeiValidationWarning>>> {
        execute_pipeline(&self.pipeline, request.into_inner())
            .await
            .map(PipelineExecuteResponse::from)
            .map(Json)
    }
}
