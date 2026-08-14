//! Restate adapters for the concrete pipeline services.

mod ingress;
mod restate;
mod workflows;

pub use ingress::{
    NewDocumentIngressClient, NewDocumentWorkflowHandle, UpdateDocumentIngressClient,
};

pub use restate::{
    DocumentRestateService, GarageRestateService, GrobidRestateService, LinkWorkflowPdfRequest,
    PipelineExecuteRequest, PipelineExecuteResponse, PublishedArtifactRestateService,
    StorePublishedArtifactRequest, TeiRestateService, TypeDbExecuteRequest, TypeDbRestateService,
    TypeDbUpdateRequest, TypeDbUpdateResponse,
};
pub use workflows::{
    DocumentExtractionWorkflow, DocumentExtractionWorkflowRequest,
    DocumentExtractionWorkflowResponse, NewDocumentWorkflow, NewDocumentWorkflowRequest,
    NewDocumentWorkflowResponse, UpdateDocumentWorkflow, UpdateDocumentWorkflowRequest,
    UpdateDocumentWorkflowResponse,
};
