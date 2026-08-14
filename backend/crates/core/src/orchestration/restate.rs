//! Thin Restate handlers around the shared pipeline lifecycle.

use restate_sdk::prelude::{Context, HandlerError, HandlerResult, Json, TerminalError};
use serde::{Deserialize, Serialize};

use crate::{
    models::{canonical::CanonicalModel, draft::TeiDocument},
    pipeline::{
        DocumentPipelineOutput, DocumentPipelineService, DocumentPipelineWarning,
        FailureDisposition, PipelineExecutionError, PipelineOutcome, PipelineService,
        garage::{GaragePipelineService, StoredPdf},
        grobid::{GrobidExtractionService, GrobidValidationWarning, HttpGrobidClient},
        tei::{TeiConversionService, TeiValidationWarning},
        typedb::{TypeDbService, TypeDbStore},
    },
    postgres::PostgresReviewStore,
};

/// The single serializable argument accepted by every pipeline handler.
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

/// JSON response returned by pipeline handlers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PipelineExecuteResponse<O, W> {
    pub output: O,
    pub warnings: Vec<W>,
}

/// Associates a Restate workflow with the content-addressed PDF it owns.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinkWorkflowPdfRequest {
    pub workflow_id: String,
    pub pdf_hash: String,
}

/// Input accepted by the TypeDB persistence service.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TypeDbExecuteRequest {
    pub pdf_hash: String,
    pub document: TeiDocument,
}

impl<O, W> From<PipelineOutcome<O, W>> for PipelineExecuteResponse<O, W> {
    fn from(outcome: PipelineOutcome<O, W>) -> Self {
        let (output, warnings) = outcome.into_parts();
        Self { output, warnings }
    }
}

/// Restate endpoint for Grobid extraction.
pub struct GrobidRestateService {
    pipeline: GrobidExtractionService<HttpGrobidClient, PostgresReviewStore>,
    pdfs: GaragePipelineService,
}

impl GrobidRestateService {
    pub fn new(
        pipeline: GrobidExtractionService<HttpGrobidClient, PostgresReviewStore>,
        pdfs: GaragePipelineService,
    ) -> Self {
        Self { pipeline, pdfs }
    }
}

#[restate_sdk::service(name = "GrobidExtractionPipeline")]
impl GrobidRestateService {
    #[restate_sdk::handler]
    async fn execute(
        &self,
        _ctx: Context<'_>,
        request: Json<PipelineExecuteRequest<String>>,
    ) -> HandlerResult<Json<PipelineExecuteResponse<String, GrobidValidationWarning>>> {
        let request = request.into_inner();
        let (_, pdf) = load_pdf(&self.pdfs, &request.input).await?;
        execute_pipeline(
            &self.pipeline,
            PipelineExecuteRequest::new(request.workflow_id, pdf),
        )
        .await
        .map(PipelineExecuteResponse::from)
        .map(Json::from)
    }
}

/// Restate endpoint for TEI-to-document conversion.
pub struct TeiRestateService {
    pipeline: TeiConversionService<PostgresReviewStore>,
}

impl TeiRestateService {
    pub fn new(pipeline: TeiConversionService<PostgresReviewStore>) -> Self {
        Self { pipeline }
    }
}

#[restate_sdk::service(name = "TeiConversionPipeline")]
impl TeiRestateService {
    #[restate_sdk::handler]
    async fn execute(
        &self,
        _ctx: Context<'_>,
        request: Json<PipelineExecuteRequest<String>>,
    ) -> HandlerResult<Json<PipelineExecuteResponse<TeiDocument, TeiValidationWarning>>> {
        execute_pipeline(&self.pipeline, request.into_inner())
            .await
            .map(PipelineExecuteResponse::from)
            .map(Json::from)
    }
}

/// Restate endpoint for the composite PDF-to-document pipeline.
pub struct DocumentRestateService {
    pipeline: DocumentPipelineService<HttpGrobidClient, PostgresReviewStore>,
    pdfs: GaragePipelineService,
}

impl DocumentRestateService {
    pub fn new(
        pipeline: DocumentPipelineService<HttpGrobidClient, PostgresReviewStore>,
        pdfs: GaragePipelineService,
    ) -> Self {
        Self { pipeline, pdfs }
    }
}

#[restate_sdk::service(name = "DocumentPipeline")]
impl DocumentRestateService {
    #[restate_sdk::handler]
    async fn execute(
        &self,
        _ctx: Context<'_>,
        request: Json<PipelineExecuteRequest<String>>,
    ) -> HandlerResult<Json<PipelineExecuteResponse<DocumentPipelineOutput, DocumentPipelineWarning>>>
    {
        let request = request.into_inner();
        let (_, pdf) = load_pdf(&self.pdfs, &request.input).await?;

        execute_pipeline(
            &self.pipeline,
            PipelineExecuteRequest::new(request.workflow_id, pdf),
        )
        .await
        .map(PipelineExecuteResponse::from)
        .map(Json::from)
    }
}

/// Restate endpoint for Garage-backed PDF ingestion.
pub struct GarageRestateService {
    pipeline: GaragePipelineService,
}

impl GarageRestateService {
    pub fn new(pipeline: GaragePipelineService) -> Self {
        Self { pipeline }
    }
}

#[restate_sdk::service(name = "GaragePipeline")]
impl GarageRestateService {
    /// Stores the workflow-to-PDF association after Garage ingestion succeeds.
    #[restate_sdk::handler]
    async fn link_workflow(
        &self,
        _ctx: Context<'_>,
        request: Json<LinkWorkflowPdfRequest>,
    ) -> HandlerResult<()> {
        let request = request.into_inner();
        self.pipeline
            .metadata()
            .link_workflow(&request.workflow_id, &request.pdf_hash)
            .await
            .map_err(to_postgres_handler_error)
    }

    /// Resolves immutable PDF metadata without putting the PDF in the journal.
    #[restate_sdk::handler]
    async fn get_pdf(
        &self,
        _ctx: Context<'_>,
        pdf_hash: Json<String>,
    ) -> HandlerResult<Json<StoredPdf>> {
        self.pipeline
            .metadata()
            .get(&pdf_hash.into_inner())
            .await
            .map_err(to_postgres_handler_error)?
            .map(Json)
            .ok_or_else(|| TerminalError::new("PDF metadata was not found").into())
    }
}

/// Restate endpoint for canonical TypeDB persistence.
pub struct TypeDbRestateService {
    service: TypeDbService<TypeDbStore>,
}

impl TypeDbRestateService {
    pub fn new(service: TypeDbService<TypeDbStore>) -> Self {
        Self { service }
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
        let canonical = self
            .service
            .pre_validate_with_pdf_hash(&request.document, &request.pdf_hash)
            .await
            .map_err(|error| TerminalError::new(error.to_string()))?;

        self.service
            .execute(&canonical)
            .await
            .map_err(|error| std::io::Error::other(error.to_string()))?;

        Ok(Json(canonical))
    }
}

async fn execute_pipeline<P>(
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

async fn load_pdf(
    pdfs: &GaragePipelineService,
    pdf_hash: &str,
) -> HandlerResult<(StoredPdf, Vec<u8>)> {
    pdfs.load(pdf_hash)
        .await
        .map_err(|error| HandlerError::from(std::io::Error::other(error.to_string())))?
        .ok_or_else(|| TerminalError::new(format!("PDF {pdf_hash} was not found")).into())
}

fn to_handler_error(error: PipelineExecutionError) -> HandlerError {
    match error.disposition() {
        FailureDisposition::Retryable => HandlerError::from(error),
        FailureDisposition::Terminal => TerminalError::new(error.to_string()).into(),
    }
}

fn to_postgres_handler_error(error: eros::ErrorUnion) -> HandlerError {
    let is_conflict = error
        .downcast_inner_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::AlreadyExists);

    if is_conflict {
        TerminalError::new(error.to_string()).into()
    } else {
        std::io::Error::other(error.to_string()).into()
    }
}
