//! Restate service adapters. Each annotated service lives in its own module.

mod artifact;
mod garage;
mod grobid;
mod tei;
mod typedb;

use restate_sdk::prelude::{HandlerError, HandlerResult, TerminalError};
use serde::{Deserialize, Serialize};

use crate::pipeline::{
    FailureDisposition, PipelineExecutionError, PipelineOutcome, PipelineService,
};

pub use artifact::{
    ArtifactRestateService, ArtifactRestateServiceClient, RepairDraft, ResolveReviewCaseRequest,
    StoreArtifactRequest, StoreTeiRequest,
};
pub use garage::{GarageRestateService, GarageRestateServiceClient, LinkWorkflowPdfRequest};
pub use grobid::{GrobidRestateService, GrobidRestateServiceClient};
pub use tei::{TeiRestateService, TeiRestateServiceClient};
pub use typedb::{
    TypeDbExecuteRequest, TypeDbRestateService, TypeDbRestateServiceClient, TypeDbUpdateRequest,
    TypeDbUpdateResponse,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PipelineExecuteRequest<I> {
    pub workflow_id: String,
    pub input: I,
}

impl<I> PipelineExecuteRequest<I> {
    pub fn new(workflow_id: impl Into<String>, input: I) -> Self {
        Self {
            workflow_id: workflow_id.into(),
            input,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PipelineExecuteResponse<O, W> {
    pub output: O,
    pub warnings: Vec<W>,
}

impl<O, W> From<PipelineOutcome<O, W>> for PipelineExecuteResponse<O, W> {
    fn from(outcome: PipelineOutcome<O, W>) -> Self {
        let (output, warnings) = outcome.into_parts();
        Self { output, warnings }
    }
}

pub(super) async fn execute_pipeline<P>(
    pipeline: &P,
    request: PipelineExecuteRequest<P::Input>,
) -> HandlerResult<PipelineOutcome<P::Output, P::Warning>>
where
    P: PipelineService,
{
    pipeline
        .execute(&request.workflow_id, &request.input)
        .await
        .map_err(to_handler_error)
}

fn to_handler_error(error: PipelineExecutionError) -> HandlerError {
    match error.disposition() {
        FailureDisposition::Retryable => HandlerError::from(error),
        FailureDisposition::Terminal => TerminalError::new(error.to_string()).into(),
    }
}

pub(super) fn to_postgres_handler_error(error: eros::ErrorUnion) -> HandlerError {
    let is_conflict = error
        .downcast_inner_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::AlreadyExists);
    if is_conflict {
        TerminalError::new(error.to_string()).into()
    } else {
        std::io::Error::other(error.to_string()).into()
    }
}
