use restate_sdk::prelude::{Context, HandlerError, HandlerResult, Json, TerminalError};
use serde::{Deserialize, Serialize};

use crate::{
    models::draft::TeiDocument,
    pipeline::vector::{DocumentVectorPipeline, VectorPipelineError},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorExecuteRequest {
    pub pdf_hash: String,
    pub document: TeiDocument,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorUpdateRequest {
    pub pdf_hash: String,
    pub old_document: TeiDocument,
    pub new_document: TeiDocument,
}

pub struct VectorRestateService {
    pipeline: DocumentVectorPipeline,
}

impl VectorRestateService {
    pub fn new(pipeline: DocumentVectorPipeline) -> Self {
        Self { pipeline }
    }
}

#[restate_sdk::service(name = "VectorPipeline")]
impl VectorRestateService {
    #[restate_sdk::handler]
    async fn execute(
        &self,
        _ctx: Context<'_>,
        request: Json<VectorExecuteRequest>,
    ) -> HandlerResult<()> {
        let request = request.into_inner();
        self.pipeline
            .publish(&request.pdf_hash, &request.document)
            .await
            .map_err(to_handler_error)
    }

    #[restate_sdk::handler]
    async fn update(
        &self,
        _ctx: Context<'_>,
        request: Json<VectorUpdateRequest>,
    ) -> HandlerResult<()> {
        let request = request.into_inner();
        self.pipeline
            .update(
                &request.pdf_hash,
                &request.old_document,
                &request.new_document,
            )
            .await
            .map_err(to_handler_error)
    }
}

fn to_handler_error(error: VectorPipelineError) -> HandlerError {
    if error.is_terminal() {
        TerminalError::new(error.to_string()).into()
    } else {
        std::io::Error::other(error.to_string()).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::draft::{Bibliography, PassageLevel};

    #[test]
    fn update_request_keeps_both_effective_documents() {
        let document = TeiDocument {
            level: PassageLevel::Paragraph,
            bibliography: Bibliography::default(),
            body_text: vec![],
            figures_and_tables: vec![],
            references: vec![],
        };
        let request = VectorUpdateRequest {
            pdf_hash: "a".repeat(64),
            old_document: document.clone(),
            new_document: document,
        };
        let value = serde_json::to_value(request).unwrap();
        assert!(value.get("old_document").is_some());
        assert!(value.get("new_document").is_some());
    }
}
