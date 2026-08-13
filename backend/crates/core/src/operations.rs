//! Shared pipeline operations used by both Clap commands and Axum handlers.

use std::{error::Error, fmt, str::FromStr};

use crate::restate::PipelineRequest;
use crate::{
    pipeline::{
        DocumentPipelineOutput, DocumentPipelineService, PipelineService,
        grobid::HttpGrobidClient,
        tei::TeiDocument,
        typedb::{TypeDbService, TypeDbStore},
    },
    postgres::PostgresReviewStore,
};
use reqwest::header;
use serde::{Deserialize, Serialize};

/// Independently invokable parts of the composite Grobid pipeline.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PipelinePart {
    InputValidation,
    OutputValidation,
    Execute,
}

impl fmt::Display for PipelinePart {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InputValidation => "input-validation",
            Self::OutputValidation => "output-validation",
            Self::Execute => "execute",
        })
    }
}

impl FromStr for PipelinePart {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "input-validation" => Ok(Self::InputValidation),
            "output-validation" => Ok(Self::OutputValidation),
            "execute" => Ok(Self::Execute),
            _ => Err(format!("unknown pipeline part: {value}")),
        }
    }
}

/// Result of invoking one pipeline part against a stored artifact.
#[derive(Debug, Serialize)]
pub struct ArtifactOperationResponse {
    pub identifier: i64,
    pub workflow_id: String,
    pub service: &'static str,
    pub part: PipelinePart,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<TeiDocument>,
}

/// Errors with an HTTP-compatible classification.
#[derive(Debug)]
pub enum OperationError {
    NotFound(String),
    Invalid(String),
    Internal(String),
}

impl fmt::Display for OperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(message) | Self::Invalid(message) | Self::Internal(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl Error for OperationError {}

/// Runs a composite Grobid operation using an existing review artifact.
pub async fn run_artifact_operation(
    store: &PostgresReviewStore,
    typedb: Option<&TypeDbService<TypeDbStore>>,
    http_client: reqwest::Client,
    grobid_url: &str,
    part: PipelinePart,
    identifier: i64,
    pdf_hash: Option<&str>,
) -> Result<ArtifactOperationResponse, OperationError> {
    let review_case = store
        .get_case(identifier)
        .await
        .map_err(internal)?
        .ok_or_else(|| OperationError::NotFound("review artifact not found".into()))?;
    let (content_type, bytes) = store
        .get_artifact(identifier)
        .await
        .map_err(internal)?
        .ok_or_else(|| OperationError::NotFound("review artifact not found".into()))?;

    let pipeline = DocumentPipelineService::new(
        HttpGrobidClient::new(http_client, grobid_url),
        store.clone(),
    );

    let (warnings, document) = match part {
        PipelinePart::InputValidation => {
            require_content_type(&content_type, "application/pdf", part)?;
            let report = pipeline
                .validate_input(&bytes)
                .await
                .map_err(invalid_pipeline)?;
            (
                report.as_slice().iter().map(ToString::to_string).collect(),
                None,
            )
        }
        PipelinePart::OutputValidation => {
            require_json_content_type(&content_type, part)?;
            let document: TeiDocument = serde_json::from_slice(&bytes).map_err(|error| {
                OperationError::Invalid(format!("artifact is not a TEI document: {error}"))
            })?;
            let output = DocumentPipelineOutput::from_document(document.clone());
            let report = pipeline
                .validate_output(&output)
                .await
                .map_err(invalid_pipeline)?;
            (
                report.as_slice().iter().map(ToString::to_string).collect(),
                Some(document),
            )
        }
        PipelinePart::Execute => {
            require_content_type(&content_type, "application/pdf", part)?;
            let outcome = pipeline
                .execute(&review_case.workflow_id, &bytes)
                .await
                .map_err(invalid_pipeline)?;
            let warnings = outcome.warnings().iter().map(ToString::to_string).collect();
            let document = outcome.into_output(|_| {}).into_document();
            if let Some(typedb) = typedb {
                let canonical = match pdf_hash {
                    Some(pdf_hash) => typedb.pre_validate_with_pdf_hash(&document, pdf_hash).await,
                    None => typedb.pre_validate(&document).await,
                }
                .map_err(invalid_pipeline)?;
                typedb.execute(&canonical).await.map_err(internal)?;
            }
            (warnings, Some(document))
        }
    };

    Ok(ArtifactOperationResponse {
        identifier,
        workflow_id: review_case.workflow_id,
        service: "grobid",
        part,
        warnings,
        document,
    })
}

/// Submits a PDF to the durable pipeline without waiting for its output.
pub async fn submit_pipeline(
    client: &reqwest::Client,
    restate_ingress_url: &str,
    identifier: &str,
    pdf_hash: &str,
) -> Result<reqwest::Response, OperationError> {
    if pdf_hash.len() != 64
        || !pdf_hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(OperationError::Invalid(
            "PDF hash must be a lowercase SHA-256 digest".into(),
        ));
    }

    let url = restate_url(
        restate_ingress_url,
        &["ScepaPipeline", identifier, "run", "send"],
    )?;
    client
        .post(url)
        .header(header::CONTENT_TYPE, "application/json")
        .json(&PipelineRequest {
            pdf_hash: pdf_hash.to_owned(),
        })
        .send()
        .await
        .map_err(|error| OperationError::Internal(format!("Restate is unavailable: {error}")))
}

pub fn restate_url(base: &str, segments: &[&str]) -> Result<reqwest::Url, OperationError> {
    let mut url = reqwest::Url::parse(base)
        .map_err(|error| OperationError::Internal(format!("invalid Restate URL: {error}")))?;
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|()| OperationError::Internal("invalid Restate URL".into()))?;
        path.pop_if_empty();
        for segment in segments {
            path.push(segment);
        }
    }
    Ok(url)
}

fn require_content_type(
    actual: &str,
    expected: &str,
    part: PipelinePart,
) -> Result<(), OperationError> {
    if actual.split(';').next() == Some(expected) {
        Ok(())
    } else {
        Err(OperationError::Invalid(format!(
            "{part} requires a {expected} artifact, found {actual}"
        )))
    }
}

fn require_json_content_type(actual: &str, part: PipelinePart) -> Result<(), OperationError> {
    if actual.split(';').next() == Some("application/json") {
        Ok(())
    } else {
        Err(OperationError::Invalid(format!(
            "{part} requires an application/json artifact, found {actual}"
        )))
    }
}

fn invalid_pipeline(error: impl fmt::Display) -> OperationError {
    OperationError::Invalid(error.to_string())
}

fn internal(error: impl fmt::Display) -> OperationError {
    OperationError::Internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_ids_are_url_encoded_as_single_segments() {
        let url = restate_url("http://localhost:8080", &["Pipeline", "paper/one", "run"]).unwrap();
        assert_eq!(
            url.as_str(),
            "http://localhost:8080/Pipeline/paper%2Fone/run"
        );
    }
}
