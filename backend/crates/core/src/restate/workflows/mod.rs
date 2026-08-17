//! Durable Restate workflows. Each annotated workflow lives in its own module.

mod document_extraction;
mod fix_document;
mod new_document;
mod update_document;

pub use document_extraction::{
    DocumentExtractionWorkflow, DocumentExtractionWorkflowClient,
    DocumentExtractionWorkflowRequest, DocumentExtractionWorkflowResponse,
};
pub use fix_document::{
    FixDocumentWorkflow, FixDocumentWorkflowRequest, FixDocumentWorkflowResponse,
};
pub use new_document::{
    NewDocumentWorkflow, NewDocumentWorkflowRequest, NewDocumentWorkflowResponse,
};
pub use update_document::{
    ReviewCaseReference, UpdateDocumentWorkflow, UpdateDocumentWorkflowClient,
    UpdateDocumentWorkflowRequest, UpdateDocumentWorkflowResponse,
};
