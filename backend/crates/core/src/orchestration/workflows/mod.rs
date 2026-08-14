//! Durable workflows for document lifecycle operations.

mod new_document;

pub use new_document::{
    DocumentExtractionWorkflow, DocumentExtractionWorkflowRequest,
    DocumentExtractionWorkflowResponse, NewDocumentWorkflow, NewDocumentWorkflowRequest,
    NewDocumentWorkflowResponse,
};
