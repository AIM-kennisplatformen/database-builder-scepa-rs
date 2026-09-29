use restate_sdk::prelude::{Context, HandlerResult, Json, TerminalError};
use serde::{Deserialize, Serialize};

use crate::{
    models::{
        canonical::CanonicalModel,
        draft::{DocumentClassification, TeiDocument},
    },
    pipeline::{
        FailureDisposition, FailureRecord, PipelinePhase, ReviewArtifact, ReviewStore,
        typedb::{CanonicalUpdateSummary, TypeDbService, TypeDbStore},
    },
    postgres::PostgresReviewStore,
};

use super::to_postgres_handler_error;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TypeDbExecuteRequest {
    pub workflow_id: String,
    pub pdf_hash: String,
    pub document: TeiDocument,
    #[serde(default)]
    pub classification: DocumentClassification,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TypeDbUpdateRequest {
    pub workflow_id: String,
    pub pdf_hash: String,
    pub old_document: TeiDocument,
    #[serde(default)]
    pub old_classification: DocumentClassification,
    pub new_document: TeiDocument,
    #[serde(default)]
    pub new_classification: DocumentClassification,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TypeDbUpdateResponse {
    pub canonical: CanonicalModel,
    pub changes: CanonicalUpdateSummary,
}

pub struct TypeDbRestateService {
    service: TypeDbService<TypeDbStore>,
    review_store: PostgresReviewStore,
}

impl TypeDbRestateService {
    pub fn new(service: TypeDbService<TypeDbStore>, review_store: PostgresReviewStore) -> Self {
        Self {
            service,
            review_store,
        }
    }

    async fn stage_invalid(
        &self,
        workflow_id: String,
        document: &TeiDocument,
        message: String,
    ) -> HandlerResult<()> {
        self.review_store
            .stage(FailureRecord {
                workflow_id,
                service: TypeDbService::<TypeDbStore>::NAME,
                phase: PipelinePhase::InputValidation,
                disposition: FailureDisposition::Terminal,
                error_message: message,
                artifact: ReviewArtifact {
                    content_type: "application/json".into(),
                    bytes: serde_json::to_vec(document).unwrap_or_default(),
                },
            })
            .await
            .map_err(to_postgres_handler_error)
    }
}

#[restate_sdk::service(name = "TypeDbPipeline")]
impl TypeDbRestateService {
    #[restate_sdk::handler]
    async fn execute(
        &self,
        _ctx: Context<'_>,
        request: Json<TypeDbExecuteRequest>,
    ) -> HandlerResult<Json<CanonicalModel>> {
        let request = request.into_inner();
        let canonical = match self
            .service
            .pre_validate_with_classification(
                &request.document,
                &request.pdf_hash,
                &request.classification,
            )
            .await
        {
            Ok(canonical) => canonical,
            Err(error) => {
                let message = error.to_string();
                self.stage_invalid(request.workflow_id, &request.document, message.clone())
                    .await?;
                return Err(TerminalError::new_with_code(
                    422,
                    "Document data is incomplete or invalid",
                )
                .into());
            }
        };
        self.service
            .execute(&canonical)
            .await
            .map_err(to_postgres_handler_error)?;
        Ok(Json(canonical))
    }

    #[restate_sdk::handler]
    async fn update(
        &self,
        _ctx: Context<'_>,
        request: Json<TypeDbUpdateRequest>,
    ) -> HandlerResult<Json<TypeDbUpdateResponse>> {
        let request = request.into_inner();
        let old = self
            .service
            .pre_validate_with_classification(
                &request.old_document,
                &request.pdf_hash,
                &request.old_classification,
            )
            .await
            .map_err(|_| TerminalError::new_with_code(422, "Existing document data is invalid"))?;
        let new = match self
            .service
            .pre_validate_with_classification(
                &request.new_document,
                &request.pdf_hash,
                &request.new_classification,
            )
            .await
        {
            Ok(canonical) => canonical,
            Err(error) => {
                let message = error.to_string();
                self.stage_invalid(request.workflow_id, &request.new_document, message.clone())
                    .await?;
                return Err(TerminalError::new_with_code(
                    422,
                    "Document data is incomplete or invalid",
                )
                .into());
            }
        };
        let changes = self
            .service
            .execute_update(&old, &new)
            .await
            .map_err(to_postgres_handler_error)?;
        Ok(Json(TypeDbUpdateResponse {
            canonical: new,
            changes,
        }))
    }
}
