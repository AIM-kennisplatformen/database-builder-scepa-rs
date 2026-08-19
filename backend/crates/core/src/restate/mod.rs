//! Restate workflows and service adapters.

pub mod client;
pub mod services;
pub mod workflows;

pub use client::{NewDocumentWorkflowHandle, RestateClient};

pub use services::{
    ArtifactRestateService, GarageRestateService, GrobidRestateService, TeiRestateService,
    TypeDbRestateService, VectorRestateService,
};
pub use workflows::{
    DocumentExtractionWorkflow, FixDocumentWorkflow, NewDocumentWorkflow, UpdateDocumentWorkflow,
};
