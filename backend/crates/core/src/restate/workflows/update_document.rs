use restate_sdk::prelude::{ContextClient, HandlerResult, Json, TerminalError, WorkflowContext};
use serde::{Deserialize, Serialize};

use crate::{
    models::{
        canonical::CanonicalModel,
        draft::{DraftDocument, ManualDocument},
    },
    pipeline::typedb::CanonicalUpdateSummary,
    restate::services::{
        ArtifactRestateServiceClient, GarageRestateServiceClient, LinkWorkflowPdfRequest,
        ResolveReviewCaseRequest, StoreArtifactRequest, TypeDbExecuteRequest,
        TypeDbRestateServiceClient, TypeDbUpdateRequest, VectorExecuteRequest,
        VectorRestateServiceClient, VectorUpdateRequest,
    },
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UpdateDocumentWorkflowRequest {
    pub pdf_hash: String,
    pub manual_data: ManualDocument,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_case: Option<ReviewCaseReference>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewCaseReference {
    pub id: i64,
    pub workflow_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UpdateDocumentWorkflowResponse {
    pub artifact: DraftDocument,
    pub canonical: CanonicalModel,
    pub changes: CanonicalUpdateSummary,
}

pub struct UpdateDocumentWorkflow;

#[restate_sdk::workflow(name = "UpdateDocumentWorkflow")]
impl UpdateDocumentWorkflow {
    #[restate_sdk::handler]
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        request: Json<UpdateDocumentWorkflowRequest>,
    ) -> HandlerResult<Json<UpdateDocumentWorkflowResponse>> {
        let request = request.into_inner();
        ctx.service_client::<GarageRestateServiceClient>()
            .link_workflow(Json(LinkWorkflowPdfRequest {
                workflow_id: ctx.key().to_owned(),
                pdf_hash: request.pdf_hash.clone(),
            }))
            .call()
            .await?;
        let old_artifact = ctx
            .service_client::<ArtifactRestateServiceClient>()
            .get_published(Json(request.pdf_hash.clone()))
            .call()
            .await?
            .into_inner();
        let source_artifact = match &old_artifact {
            Some(artifact) => artifact.clone(),
            None => ctx
                .service_client::<ArtifactRestateServiceClient>()
                .get_draft(Json(request.pdf_hash.clone()))
                .call()
                .await?
                .into_inner()
                .ok_or_else(|| TerminalError::new("document artifact was not found"))?,
        };
        let mut new_artifact = source_artifact;
        new_artifact.manual_data = request.manual_data;
        ctx.service_client::<ArtifactRestateServiceClient>()
            .store_draft(Json(StoreArtifactRequest {
                pdf_hash: request.pdf_hash.clone(),
                artifact: new_artifact.clone(),
            }))
            .call()
            .await?;

        let new_document = new_artifact.effective_document();
        let (canonical, changes) = if let Some(old_artifact) = old_artifact {
            let old_document = old_artifact.effective_document();
            let updated = ctx
                .service_client::<TypeDbRestateServiceClient>()
                .update(Json(TypeDbUpdateRequest {
                    workflow_id: ctx.key().to_owned(),
                    pdf_hash: request.pdf_hash.clone(),
                    old_document: old_document.clone(),
                    new_document: new_document.clone(),
                }))
                .call()
                .await?
                .into_inner();
            ctx.service_client::<VectorRestateServiceClient>()
                .update(Json(VectorUpdateRequest {
                    pdf_hash: request.pdf_hash.clone(),
                    old_document,
                    new_document: new_document.clone(),
                }))
                .call()
                .await?;
            (updated.canonical, updated.changes)
        } else {
            let canonical = ctx
                .service_client::<TypeDbRestateServiceClient>()
                .execute(Json(TypeDbExecuteRequest {
                    workflow_id: request
                        .review_case
                        .as_ref()
                        .map(|review| review.workflow_id.clone())
                        .unwrap_or_else(|| ctx.key().to_owned()),
                    pdf_hash: request.pdf_hash.clone(),
                    document: new_document.clone(),
                }))
                .call()
                .await?
                .into_inner();
            ctx.service_client::<VectorRestateServiceClient>()
                .execute(Json(VectorExecuteRequest {
                    pdf_hash: request.pdf_hash.clone(),
                    document: new_document.clone(),
                }))
                .call()
                .await?;
            let changes = CanonicalUpdateSummary {
                document_changed: true,
                contributors_inserted: canonical.persons.len(),
                organizations_inserted: canonical.organizations.len(),
                venues_inserted: canonical.publication_venues.len(),
                affiliations_inserted: canonical.affiliations.len(),
                publication_events_inserted: canonical.publication_events.len(),
                ..CanonicalUpdateSummary::default()
            };
            (canonical, changes)
        };
        ctx.service_client::<ArtifactRestateServiceClient>()
            .store_published(Json(StoreArtifactRequest {
                pdf_hash: request.pdf_hash.clone(),
                artifact: new_artifact.clone(),
            }))
            .call()
            .await?;
        if let Some(review_case) = request.review_case {
            ctx.service_client::<ArtifactRestateServiceClient>()
                .resolve_review_case(Json(ResolveReviewCaseRequest {
                    case_id: review_case.id,
                    workflow_id: review_case.workflow_id,
                    pdf_hash: request.pdf_hash,
                }))
                .call()
                .await?;
        }
        Ok(Json(UpdateDocumentWorkflowResponse {
            artifact: new_artifact,
            canonical,
            changes,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_updates_do_not_require_a_review_case() {
        let request: UpdateDocumentWorkflowRequest = serde_json::from_value(serde_json::json!({
            "pdf_hash": "a".repeat(64),
            "manual_data": { "bibliography": {} }
        }))
        .unwrap();
        assert_eq!(request.review_case, None);
    }
}
