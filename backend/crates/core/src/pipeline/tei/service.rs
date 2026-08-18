//! Pipeline service for typed TEI conversion.

use std::{error::Error, fmt};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::parser;
use crate::models::draft::{Passage, TeiDocument};
use crate::pipeline::{PipelineService, ReviewArtifact, ReviewStore, ValidationReport};

/// Non-fatal quality findings produced by the TEI conversion stage.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeiValidationWarning {
    MissingTitle,
    EmptyBody,
}

impl fmt::Display for TeiValidationWarning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MissingTitle => "converted TEI contains no document title",
            Self::EmptyBody => "converted TEI contains no body passages",
        })
    }
}

impl Error for TeiValidationWarning {}

/// Pipeline stage converting TEI XML into [`TeiDocument`].
pub struct TeiConversionService<S> {
    review_store: S,
}

impl<S> TeiConversionService<S> {
    pub fn new(review_store: S) -> Self {
        Self { review_store }
    }

    /// Converts a document without running the pipeline validation lifecycle.
    pub fn convert(&self, tei: &str) -> eros::Result<TeiDocument> {
        parser::convert_tei(tei)
    }
}

#[async_trait]
impl<S> PipelineService for TeiConversionService<S>
where
    S: ReviewStore,
{
    type Input = String;
    type Output = TeiDocument;
    type Warning = TeiValidationWarning;

    const NAME: &'static str = "tei-conversion";

    fn review_store(&self) -> &dyn ReviewStore {
        &self.review_store
    }

    async fn validate_input(
        &self,
        tei: &Self::Input,
    ) -> eros::Result<ValidationReport<Self::Warning>> {
        if tei.trim().is_empty() {
            eros::bail!("cannot convert an empty TEI document")
        }
        Ok(ValidationReport::clean())
    }

    async fn process(&self, tei: &Self::Input) -> eros::Result<Self::Output> {
        self.convert(tei)
    }

    async fn validate_output(
        &self,
        document: &Self::Output,
    ) -> eros::Result<ValidationReport<Self::Warning>> {
        for passage in document
            .body_text
            .iter()
            .filter_map(|passage| match passage {
                Passage::Text(passage) => Some(passage),
                Passage::Formula(_) => None,
            })
            .chain(document.bibliography.abstract_text.iter())
        {
            for reference in &passage.references {
                if passage.text.get(reference.byte_start..reference.byte_end)
                    != Some(reference.text.as_str())
                {
                    eros::bail!(
                        "reference offsets are invalid in passage {} for target {}",
                        passage.id,
                        reference.target.as_deref().unwrap_or("<unknown>")
                    )
                }
            }
        }

        let mut warnings = Vec::new();
        if document.bibliography.title.is_none() {
            warnings.push(TeiValidationWarning::MissingTitle);
        }
        if document.body_text.is_empty() {
            warnings.push(TeiValidationWarning::EmptyBody);
        }
        Ok(ValidationReport::warnings(warnings))
    }

    fn review_artifact(&self, tei: &Self::Input, output: Option<&Self::Output>) -> ReviewArtifact {
        match output {
            Some(document) => ReviewArtifact {
                content_type: "application/json".into(),
                bytes: serde_json::to_vec(document).unwrap_or_default(),
            },
            None => ReviewArtifact {
                content_type: "application/tei+xml".into(),
                bytes: tei.as_bytes().to_vec(),
            },
        }
    }
}
