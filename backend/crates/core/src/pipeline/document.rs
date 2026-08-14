//! Composite PDF-to-document pipeline service.

use std::{error::Error, fmt};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    FailureDisposition, PipelinePhase, PipelineService, ReviewArtifact, ReviewStore,
    ValidationReport,
    garage::sha256_hex,
    grobid::{GrobidClient, GrobidExtractionService, GrobidValidationWarning},
    tei::{TeiConversionService, TeiDocument, TeiValidationWarning},
};

/// PostgreSQL-facing durability boundary for extracted document data.
#[async_trait]
pub trait DocumentArtifactStore: Send + Sync {
    async fn store_tei_xml(&self, pdf_hash: &str, tei_xml: &str) -> eros::Result<()>;
    async fn store_draft_artifact(&self, pdf_hash: &str, draft: &TeiDocument) -> eros::Result<()>;
}

/// Warning emitted by either stage of the composite document pipeline.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "stage", content = "warning", rename_all = "snake_case")]
pub enum DocumentPipelineWarning {
    Grobid(GrobidValidationWarning),
    Tei(TeiValidationWarning),
}

impl fmt::Display for DocumentPipelineWarning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Grobid(warning) => warning.fmt(formatter),
            Self::Tei(warning) => warning.fmt(formatter),
        }
    }
}

impl Error for DocumentPipelineWarning {}

/// Output carried between the composite processing and output-validation phases.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentPipelineOutput {
    tei: String,
    document: TeiDocument,
    processing_warnings: Vec<DocumentPipelineWarning>,
}

impl DocumentPipelineOutput {
    /// Wraps a previously persisted document for independent output validation.
    pub fn from_document(document: TeiDocument) -> Self {
        Self {
            tei: String::new(),
            document,
            processing_warnings: Vec::new(),
        }
    }

    pub fn document(&self) -> &TeiDocument {
        &self.document
    }

    /// Returns the raw TEI XML produced by Grobid.
    pub fn tei(&self) -> &str {
        &self.tei
    }

    pub fn into_document(self) -> TeiDocument {
        self.document
    }
}

#[derive(Clone, Copy, Debug)]
enum DocumentProcessingStage {
    GrobidProcessing,
    TeiPersistence,
    GrobidOutputValidation,
    TeiInputValidation,
    TeiConversion,
    DraftPersistence,
}

#[derive(Debug)]
struct DocumentProcessingError {
    stage: DocumentProcessingStage,
    disposition: FailureDisposition,
    message: String,
}

impl fmt::Display for DocumentProcessingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?} failed: {}", self.stage, self.message)
    }
}

impl Error for DocumentProcessingError {}

/// Runs Grobid extraction followed by typed TEI conversion as one service.
pub struct DocumentPipelineService<C, S> {
    grobid: GrobidExtractionService<C, S>,
    conversion: TeiConversionService<S>,
    review_store: S,
}

impl<C, S> DocumentPipelineService<C, S>
where
    S: Clone,
{
    pub fn new(client: C, review_store: S) -> Self {
        Self {
            grobid: GrobidExtractionService::new(client, review_store.clone()),
            conversion: TeiConversionService::new(review_store.clone()),
            review_store,
        }
    }
}

#[async_trait]
impl<C, S> PipelineService for DocumentPipelineService<C, S>
where
    C: GrobidClient,
    S: DocumentArtifactStore + ReviewStore + Clone,
{
    type Input = Vec<u8>;
    type Output = DocumentPipelineOutput;
    type Warning = DocumentPipelineWarning;

    const NAME: &'static str = "document-pipeline";

    fn review_store(&self) -> &dyn ReviewStore {
        &self.review_store
    }

    fn failure_disposition(
        &self,
        phase: PipelinePhase,
        error: &eros::ErrorUnion,
    ) -> FailureDisposition {
        if phase != PipelinePhase::Processing {
            return FailureDisposition::Terminal;
        }

        error
            .downcast_inner_ref::<DocumentProcessingError>()
            .map_or(FailureDisposition::Terminal, |error| error.disposition)
    }

    async fn validate_input(
        &self,
        pdf: &Self::Input,
    ) -> eros::Result<ValidationReport<Self::Warning>> {
        let report = self.grobid.validate_input(pdf).await?;
        Ok(ValidationReport::warnings(
            report
                .as_slice()
                .iter()
                .copied()
                .map(DocumentPipelineWarning::Grobid),
        ))
    }

    async fn process(&self, pdf: &Self::Input) -> eros::Result<Self::Output> {
        let pdf_hash = sha256_hex(pdf);
        let tei = match self.grobid.process(pdf).await {
            Ok(tei) => tei,
            Err(error) => {
                let disposition = self
                    .grobid
                    .failure_disposition(PipelinePhase::Processing, &error);
                return Err(document_error(
                    DocumentProcessingStage::GrobidProcessing,
                    disposition,
                    error,
                ));
            }
        };

        self.review_store
            .store_tei_xml(&pdf_hash, &tei)
            .await
            .map_err(|error| {
                document_error(
                    DocumentProcessingStage::TeiPersistence,
                    FailureDisposition::Retryable,
                    error,
                )
            })?;

        let grobid_report = self.grobid.validate_output(&tei).await.map_err(|error| {
            document_error(
                DocumentProcessingStage::GrobidOutputValidation,
                FailureDisposition::Terminal,
                error,
            )
        })?;

        if let Err(error) = self.conversion.validate_input(&tei).await {
            return Err(document_error(
                DocumentProcessingStage::TeiInputValidation,
                FailureDisposition::Terminal,
                error,
            ));
        }

        let document = self.conversion.process(&tei).await.map_err(|error| {
            document_error(
                DocumentProcessingStage::TeiConversion,
                FailureDisposition::Terminal,
                error,
            )
        })?;

        self.review_store
            .store_draft_artifact(&pdf_hash, &document)
            .await
            .map_err(|error| {
                document_error(
                    DocumentProcessingStage::DraftPersistence,
                    FailureDisposition::Retryable,
                    error,
                )
            })?;

        tracing::debug!(
            tei_bytes = tei.len(),
            body_passages = document.body_text.len(),
            "parsed Grobid TEI into the typed document model"
        );

        Ok(DocumentPipelineOutput {
            tei,
            document,
            processing_warnings: grobid_report
                .as_slice()
                .iter()
                .copied()
                .map(DocumentPipelineWarning::Grobid)
                .collect(),
        })
    }

    async fn validate_output(
        &self,
        output: &Self::Output,
    ) -> eros::Result<ValidationReport<Self::Warning>> {
        let report = self.conversion.validate_output(&output.document).await?;
        let warnings = output
            .processing_warnings
            .iter()
            .copied()
            .chain(
                report
                    .as_slice()
                    .iter()
                    .copied()
                    .map(DocumentPipelineWarning::Tei),
            )
            .collect::<Vec<_>>();
        Ok(ValidationReport::warnings(warnings))
    }

    fn review_artifact(&self, pdf: &Self::Input, output: Option<&Self::Output>) -> ReviewArtifact {
        match output {
            Some(output) => ReviewArtifact {
                content_type: "application/json".into(),
                bytes: serde_json::to_vec(&output.document).unwrap_or_default(),
            },
            None => ReviewArtifact {
                content_type: "application/pdf".into(),
                bytes: pdf.clone(),
            },
        }
    }
}

fn document_error(
    stage: DocumentProcessingStage,
    disposition: FailureDisposition,
    error: eros::ErrorUnion,
) -> eros::ErrorUnion {
    DocumentProcessingError {
        stage,
        disposition,
        message: error.to_string(),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::pipeline::FailureRecord;

    #[derive(Clone, Default)]
    struct RecordingStore {
        failures: Arc<Mutex<Vec<FailureRecord>>>,
        teis: Arc<Mutex<Vec<(String, String)>>>,
        drafts: Arc<Mutex<Vec<(String, TeiDocument)>>>,
    }

    #[async_trait]
    impl ReviewStore for RecordingStore {
        async fn stage(&self, failure: FailureRecord) -> eros::Result<()> {
            self.failures.lock().unwrap().push(failure);
            Ok(())
        }
    }

    #[async_trait]
    impl DocumentArtifactStore for RecordingStore {
        async fn store_tei_xml(&self, pdf_hash: &str, tei_xml: &str) -> eros::Result<()> {
            self.teis
                .lock()
                .unwrap()
                .push((pdf_hash.to_owned(), tei_xml.to_owned()));
            Ok(())
        }

        async fn store_draft_artifact(
            &self,
            pdf_hash: &str,
            draft: &TeiDocument,
        ) -> eros::Result<()> {
            self.drafts
                .lock()
                .unwrap()
                .push((pdf_hash.to_owned(), draft.clone()));
            Ok(())
        }
    }

    struct FailingClient;

    #[async_trait]
    impl GrobidClient for FailingClient {
        async fn extract_tei(&self, _pdf: &[u8]) -> eros::Result<String> {
            Err(std::io::Error::other("invalid local Grobid response").into())
        }
    }

    struct InvalidTeiClient;

    #[async_trait]
    impl GrobidClient for InvalidTeiClient {
        async fn extract_tei(&self, _pdf: &[u8]) -> eros::Result<String> {
            Ok("<invalid/>".into())
        }
    }

    struct ValidTeiClient;

    #[async_trait]
    impl GrobidClient for ValidTeiClient {
        async fn extract_tei(&self, _pdf: &[u8]) -> eros::Result<String> {
            Ok(r#"<TEI><teiHeader><fileDesc><titleStmt><title>Stored</title></titleStmt><publicationStmt><p/></publicationStmt><sourceDesc><p/></sourceDesc></fileDesc></teiHeader><text><body><p>Body</p></body></text></TEI>"#.into())
        }
    }

    #[test]
    fn processing_disposition_is_preserved_by_the_wrapper_error() {
        let error = document_error(
            DocumentProcessingStage::GrobidProcessing,
            FailureDisposition::Retryable,
            std::io::Error::other("temporarily unavailable").into(),
        );

        let tagged = error
            .downcast_inner_ref::<DocumentProcessingError>()
            .unwrap();
        assert_eq!(tagged.disposition, FailureDisposition::Retryable);
    }

    #[test]
    fn conversion_errors_are_terminal() {
        let error = document_error(
            DocumentProcessingStage::TeiConversion,
            FailureDisposition::Terminal,
            std::io::Error::other("malformed TEI").into(),
        );

        let tagged = error
            .downcast_inner_ref::<DocumentProcessingError>()
            .unwrap();
        assert_eq!(tagged.disposition, FailureDisposition::Terminal);
    }

    #[tokio::test]
    async fn unclassified_grobid_processing_errors_are_staged_as_terminal() {
        let store = RecordingStore::default();
        let service = DocumentPipelineService::new(FailingClient, store.clone());

        assert!(
            service
                .execute("workflow-1", &b"pdf".to_vec())
                .await
                .is_err()
        );

        let failures = store.failures.lock().unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].service, "document-pipeline");
        assert_eq!(failures[0].phase, PipelinePhase::Processing);
        assert_eq!(failures[0].disposition, FailureDisposition::Terminal);
    }

    #[tokio::test]
    async fn tei_is_persisted_before_later_validation_can_fail() {
        let store = RecordingStore::default();
        let service = DocumentPipelineService::new(InvalidTeiClient, store.clone());
        let pdf = b"pdf".to_vec();

        assert!(service.execute("workflow-1", &pdf).await.is_err());

        let teis = store.teis.lock().unwrap();
        assert_eq!(teis.as_slice(), &[(sha256_hex(&pdf), "<invalid/>".into())]);
        assert!(store.drafts.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn successful_extraction_persists_tei_and_draft() {
        let store = RecordingStore::default();
        let service = DocumentPipelineService::new(ValidTeiClient, store.clone());
        let pdf = b"pdf".to_vec();

        let _ = service.execute("workflow-1", &pdf).await.unwrap();

        assert_eq!(store.teis.lock().unwrap().len(), 1);
        assert_eq!(store.drafts.lock().unwrap().len(), 1);
    }
}
