//! Durable workflows for document lifecycle operations.

mod new_document;
mod update_document;

pub use new_document::{
    DocumentExtractionWorkflow, DocumentExtractionWorkflowRequest,
    DocumentExtractionWorkflowResponse, NewDocumentWorkflow, NewDocumentWorkflowRequest,
    NewDocumentWorkflowResponse,
};
pub use update_document::{
    UpdateDocumentWorkflow, UpdateDocumentWorkflowRequest, UpdateDocumentWorkflowResponse,
};
