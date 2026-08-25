//! Grobid implementation of the generic pipeline contract.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

use crate::pipeline::{
    FailureDisposition, PipelinePhase, PipelineService, ReviewArtifact, ReviewStore,
    ValidationReport,
};

const TEI_COORDINATE_ELEMENTS: &[&str] = &[
    "p",
    "s",
    "figure",
    "ref",
    "biblStruct",
    "formula",
    "persName",
];

/// Boundary around Grobid's HTTP API.
///
/// Keep the HTTP client outside the pipeline service so it can be configured
/// with application-specific timeouts, authentication, and retry policy, and
/// so the pipeline service remains straightforward to test.
#[async_trait]
pub trait GrobidClient: Send + Sync {
    /// Extracts TEI XML from a PDF document.
    async fn extract_tei(&self, pdf: &[u8]) -> eros::Result<String>;
}

/// HTTP client for Grobid's full-text document endpoint.
#[derive(Clone)]
pub struct HttpGrobidClient {
    client: reqwest::Client,
    endpoint: String,
}

impl HttpGrobidClient {
    /// Creates a client for a Grobid base URL such as `http://localhost:8070`.
    pub fn new(client: reqwest::Client, base_url: impl AsRef<str>) -> Self {
        Self {
            client,
            endpoint: format!(
                "{}/api/processFulltextDocument",
                base_url.as_ref().trim_end_matches('/')
            ),
        }
    }
}

#[async_trait]
impl GrobidClient for HttpGrobidClient {
    async fn extract_tei(&self, pdf: &[u8]) -> eros::Result<String> {
        let input = reqwest::multipart::Part::bytes(pdf.to_vec())
            .file_name("input.pdf")
            .mime_str("application/pdf")?;

        let form = reqwest::multipart::Form::new()
            .part("input", input)
            .text("generateIDS", "1")
            .text("includeRawCitations", "0")
            .text("includeRawAffiliations", "1");
        let form = TEI_COORDINATE_ELEMENTS
            .iter()
            .fold(form, |form, element| form.text("teiCoordinates", *element));

        let response = self
            .client
            .post(&self.endpoint)
            .multipart(form)
            .send()
            .await?
            .error_for_status()?;

        Ok(response.text().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::TEI_COORDINATE_ELEMENTS;

    #[test]
    fn requests_coordinates_for_each_text_chunk_level() {
        assert!(TEI_COORDINATE_ELEMENTS.contains(&"p"));
        assert!(TEI_COORDINATE_ELEMENTS.contains(&"s"));
    }
}

/// Pipeline service that submits PDFs to Grobid and validates their TEI output.
pub struct GrobidExtractionService<C, S> {
    client: C,
    review_store: S,
}

/// Non-fatal findings emitted while validating a Grobid response.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GrobidValidationWarning {
    /// The extracted TEI does not contain a title element.
    MissingTitle,
}

impl fmt::Display for GrobidValidationWarning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTitle => {
                formatter.write_str("TEI extraction succeeded but contains no title element")
            }
        }
    }
}

impl Error for GrobidValidationWarning {}

impl<C, S> GrobidExtractionService<C, S> {
    /// Creates a Grobid extraction service using the supplied API client and
    /// durable review store.
    pub fn new(client: C, review_store: S) -> Self {
        Self {
            client,
            review_store,
        }
    }
}

#[async_trait]
impl<C, S> PipelineService for GrobidExtractionService<C, S>
where
    C: GrobidClient,
    S: ReviewStore,
{
    type Input = Vec<u8>;
    type Output = String;
    type Warning = GrobidValidationWarning;

    const NAME: &'static str = "grobid-extraction";

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

        let Some(error) = error.downcast_inner_ref::<reqwest::Error>() else {
            return FailureDisposition::Terminal;
        };

        if error.is_connect() || error.is_timeout() {
            return FailureDisposition::Retryable;
        }

        match error.status() {
            Some(status)
                if status.is_server_error()
                    || status == reqwest::StatusCode::REQUEST_TIMEOUT
                    || status == reqwest::StatusCode::TOO_MANY_REQUESTS =>
            {
                FailureDisposition::Retryable
            }
            _ => FailureDisposition::Terminal,
        }
    }

    async fn validate_input(
        &self,
        pdf: &Self::Input,
    ) -> eros::Result<ValidationReport<Self::Warning>> {
        if pdf.is_empty() {
            eros::bail!("cannot extract TEI from an empty PDF")
        }
        Ok(ValidationReport::clean())
    }

    async fn process(&self, pdf: &Self::Input) -> eros::Result<Self::Output> {
        tracing::debug!(pdf_bytes = pdf.len(), "submitting PDF to Grobid");
        let tei = self.client.extract_tei(pdf).await?;
        tracing::debug!(tei_bytes = tei.len(), "received TEI XML from Grobid");
        Ok(tei)
    }

    async fn validate_output(
        &self,
        tei: &Self::Output,
    ) -> eros::Result<ValidationReport<Self::Warning>> {
        if !tei.contains("<TEI") {
            eros::bail!("Grobid response is not a TEI document")
        }
        if !tei.contains("<title") {
            return Ok(ValidationReport::warning(
                GrobidValidationWarning::MissingTitle,
            ));
        }
        Ok(ValidationReport::clean())
    }

    fn review_artifact(&self, pdf: &Self::Input, output: Option<&Self::Output>) -> ReviewArtifact {
        match output {
            Some(tei) => ReviewArtifact {
                content_type: "application/tei+xml".into(),
                bytes: tei.as_bytes().to_vec(),
            },
            None => ReviewArtifact {
                content_type: "application/pdf".into(),
                bytes: pdf.clone(),
            },
        }
    }
}
