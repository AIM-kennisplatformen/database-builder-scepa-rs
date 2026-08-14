//! Restate adapters for the concrete pipeline services.

mod restate;
mod workflows;

pub use restate::{
    DocumentRestateService, GarageRestateService, GrobidRestateService, LinkWorkflowPdfRequest,
    PipelineExecuteRequest, PipelineExecuteResponse, TeiRestateService, TypeDbExecuteRequest,
    TypeDbRestateService,
};
pub use workflows::{
    DocumentExtractionWorkflow, DocumentExtractionWorkflowRequest,
    DocumentExtractionWorkflowResponse, NewDocumentWorkflow, NewDocumentWorkflowRequest,
    NewDocumentWorkflowResponse,
};
