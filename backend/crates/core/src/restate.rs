//! Restate workflow exposing the composite document pipeline as a durable step.

use std::{path::PathBuf, time::Duration};

use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    pipeline::{
        DocumentPipelineOutput, DocumentPipelineService, FailureDisposition, FailureRecord,
        PipelinePhase, PipelineService, ReviewArtifact, ReviewStore,
        garage::GaragePipelineService,
        grobid::HttpGrobidClient,
        tei::TeiDocument,
        typedb::{TypeDbService, TypeDbStore},
    },
    postgres::PostgresReviewStore,
};

const PROCESSING_REVIEW_PROMISE: &str = "document-processing-review";
const TERMINAL_PROCESSING_CODE: u16 = 422;

type Pipeline = DocumentPipelineService<HttpGrobidClient, PostgresReviewStore>;

/// Successful result returned by the durable document workflow.
#[derive(Debug, Serialize, Deserialize)]
pub struct PipelineResponse {
    pub pdf_hash: String,
    pub document: TeiDocument,
    pub warnings: Vec<String>,
}

/// The durable workflow carries only immutable storage identity, not PDF bytes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PipelineRequest {
    pub pdf_hash: String,
}

/// Operator decision for a staged review case.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    Retry,
    Abort,
}

impl ReviewDecision {
    pub const fn status(self) -> &'static str {
        match self {
            Self::Retry => "retry_requested",
            Self::Abort => "aborted",
        }
    }
}

/// Payload accepted by the workflow's shared review-resolution handler.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResolveReviewRequest {
    pub review_case_id: i64,
    pub decision: ReviewDecision,
}

/// Durable PDF extraction and TEI conversion workflow.
pub struct ScepaWorkflow {
    pipeline: Pipeline,
    review_store: PostgresReviewStore,
    typedb: TypeDbService<TypeDbStore>,
    garage_pipeline: GaragePipelineService,
    debug_artifact_root: PathBuf,
}

impl ScepaWorkflow {
    pub fn new(
        grobid_client: HttpGrobidClient,
        review_store: PostgresReviewStore,
        typedb: TypeDbService<TypeDbStore>,
        garage_pipeline: GaragePipelineService,
    ) -> Self {
        Self {
            pipeline: DocumentPipelineService::new(grobid_client, review_store.clone()),
            review_store,
            typedb,
            garage_pipeline,
            debug_artifact_root: PathBuf::from(".artifacts"),
        }
    }

    /// Selects where successful raw TEI and parsed JSON debug artifacts live.
    pub fn with_debug_artifact_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.debug_artifact_root = root.into();
        self
    }
}

#[restate_sdk::workflow(name = "ScepaPipeline")]
impl ScepaWorkflow {
    /// Runs the pipeline from a PDF already stored under its SHA-256 identity.
    #[handler]
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        request: Json<PipelineRequest>,
    ) -> Result<Json<PipelineResponse>, HandlerError> {
        let workflow_id = ctx.key().to_owned();
        let pdf_hash = request.into_inner().pdf_hash;
        let mut warnings = Vec::new();

        let repository = self.garage_pipeline.clone();
        let stored_hash = pdf_hash.clone();
        let pdf = ctx
            .run(|| async move {
                match repository.load(&stored_hash).await {
                    Ok(Some((_, bytes))) => Ok(Json::from(bytes)),
                    Ok(None) => Err(TerminalError::new_with_code(
                        404,
                        format!("PDF {stored_hash} is not indexed in PostgreSQL"),
                    )
                    .into()),
                    Err(error) => Err(retryable_error(error)),
                }
            })
            .name("load-pdf-from-garage")
            .await?
            .into_inner();

        match self.pipeline.validate_input(&pdf).await {
            Ok(report) => warnings.extend(report.as_slice().iter().map(ToString::to_string)),
            Err(error) => {
                let message = error.to_string();
                stage_failure(
                    &ctx,
                    self.review_store.clone(),
                    FailureRecord {
                        workflow_id,
                        service: Pipeline::NAME,
                        phase: PipelinePhase::InputValidation,
                        disposition: FailureDisposition::Terminal,
                        error_message: message.clone(),
                        artifact: self.pipeline.review_artifact(&pdf, None),
                    },
                )
                .await?;
                return Err(TerminalError::new_with_code(400, message).into());
            }
        }

        let retry_policy = RunRetryPolicy::new()
            .initial_delay(Duration::from_millis(250))
            .exponentiation_factor(2.0)
            .max_delay(Duration::from_secs(10))
            .max_attempts(5);

        let processing_pdf = pdf.clone();
        let output = match ctx
            .run(|| async move {
                run_pipeline_process(&self.pipeline, processing_pdf)
                    .await
                    .map(Json::from)
            })
            .name("grobid-and-tei")
            .retry_policy(retry_policy)
            .await
        {
            Ok(output) => output.into_inner(),
            Err(error) => {
                let disposition = if error.code() == TERMINAL_PROCESSING_CODE {
                    FailureDisposition::Terminal
                } else {
                    FailureDisposition::Retryable
                };
                stage_failure(
                    &ctx,
                    self.review_store.clone(),
                    FailureRecord {
                        workflow_id: workflow_id.clone(),
                        service: Pipeline::NAME,
                        phase: PipelinePhase::Processing,
                        disposition,
                        error_message: error.message().to_owned(),
                        artifact: self.pipeline.review_artifact(&pdf, None),
                    },
                )
                .await?;

                if disposition == FailureDisposition::Terminal {
                    return Err(error.into());
                }

                let decision = ctx
                    .promise::<Json<ReviewDecision>>(PROCESSING_REVIEW_PROMISE)
                    .await?
                    .into_inner();
                match decision {
                    ReviewDecision::Retry => {
                        let retry_pdf = pdf.clone();
                        ctx.run(|| async move {
                            run_pipeline_process(&self.pipeline, retry_pdf)
                                .await
                                .map(Json::from)
                        })
                        .name("grobid-and-tei-after-review")
                        .await?
                        .into_inner()
                    }
                    ReviewDecision::Abort => return Err(error.into()),
                }
            }
        };

        match self.pipeline.validate_output(&output).await {
            Ok(report) => warnings.extend(report.as_slice().iter().map(ToString::to_string)),
            Err(error) => {
                let message = error.to_string();
                stage_failure(
                    &ctx,
                    self.review_store.clone(),
                    FailureRecord {
                        workflow_id,
                        service: Pipeline::NAME,
                        phase: PipelinePhase::OutputValidation,
                        disposition: FailureDisposition::Terminal,
                        error_message: message.clone(),
                        artifact: self.pipeline.review_artifact(&pdf, Some(&output)),
                    },
                )
                .await?;
                return Err(TerminalError::new_with_code(422, message).into());
            }
        }

        let canonical = match self
            .typedb
            .pre_validate_with_pdf_hash(output.document(), &pdf_hash)
            .await
        {
            Ok(canonical) => canonical,
            Err(error) => {
                let message = error.to_string();
                stage_failure(
                    &ctx,
                    self.review_store.clone(),
                    FailureRecord {
                        workflow_id: workflow_id.clone(),
                        service: TypeDbService::<TypeDbStore>::NAME,
                        phase: PipelinePhase::InputValidation,
                        disposition: FailureDisposition::Terminal,
                        error_message: message.clone(),
                        artifact: ReviewArtifact {
                            content_type: "application/json".into(),
                            bytes: serde_json::to_vec(output.document()).unwrap_or_default(),
                        },
                    },
                )
                .await?;
                return Err(TerminalError::new_with_code(422, message).into());
            }
        };

        let typedb = self.typedb.clone();
        ctx.run(|| async move { typedb.execute(&canonical).await.map_err(retryable_error) })
            .name("typedb-insert")
            .retry_policy(
                RunRetryPolicy::new()
                    .initial_delay(Duration::from_millis(250))
                    .exponentiation_factor(2.0)
                    .max_delay(Duration::from_secs(10))
                    .max_attempts(5),
            )
            .await?;

        let artifact_root = self.debug_artifact_root.clone();
        let artifact_id = workflow_id.clone();
        let artifact_output = output.clone();
        ctx.run(|| async move {
            write_debug_artifacts(&artifact_root, &artifact_id, &artifact_output)
                .await
                .map_err(retryable_error)
        })
        .name("write-debug-artifacts")
        .await?;

        Ok(Json::from(PipelineResponse {
            pdf_hash,
            document: output.into_document(),
            warnings,
        }))
    }

    /// Resolves a pending human-review decision and wakes the workflow.
    #[handler]
    async fn resolve_review(
        &self,
        ctx: SharedWorkflowContext<'_>,
        request: Json<ResolveReviewRequest>,
    ) -> Result<(), HandlerError> {
        let request = request.into_inner();
        let workflow_id = ctx.key().to_owned();
        let status = request.decision.status();
        let resolution = serde_json::to_value(&request).map_err(TerminalError::from_error)?;
        let store = self.review_store.clone();
        let review_case_id = request.review_case_id;

        let updated = ctx
            .run(|| async move {
                store
                    .resolve_case(review_case_id, &workflow_id, status, resolution)
                    .await
                    .map_err(retryable_error)
            })
            .name("resolve-review-case")
            .await?;

        if !updated {
            return Err(TerminalError::new_with_code(
                409,
                "review case is missing, belongs to another workflow, or is already resolved",
            )
            .into());
        }

        ctx.resolve_promise(PROCESSING_REVIEW_PROMISE, Json::from(request.decision));
        Ok(())
    }
}

async fn write_debug_artifacts(
    root: &std::path::Path,
    workflow_id: &str,
    output: &DocumentPipelineOutput,
) -> std::io::Result<()> {
    let identifier = artifact_filename(workflow_id);
    let tei_directory = root.join("tei");
    let json_directory = root.join("json");
    tokio::fs::create_dir_all(&tei_directory).await?;
    tokio::fs::create_dir_all(&json_directory).await?;

    let tei_path = tei_directory.join(format!("{identifier}.tei.xml"));
    let json_path = json_directory.join(format!("{identifier}.json"));
    let json = serde_json::to_vec_pretty(output.document()).map_err(std::io::Error::other)?;
    tokio::fs::write(&tei_path, output.tei()).await?;
    tokio::fs::write(&json_path, json).await?;

    tracing::debug!(
        workflow_id,
        tei_path = %tei_path.display(),
        json_path = %json_path.display(),
        "wrote pipeline debug artifacts"
    );
    Ok(())
}

/// Deletes the generated TEI and JSON debug artifacts for a workflow.
///
/// Missing files are ignored so callers can safely retry the operation.
pub async fn delete_debug_artifacts(
    root: &std::path::Path,
    workflow_id: &str,
) -> std::io::Result<usize> {
    let identifier = artifact_filename(workflow_id);
    let paths = [
        root.join("tei").join(format!("{identifier}.tei.xml")),
        root.join("json").join(format!("{identifier}.json")),
    ];
    let mut deleted = 0;

    for path in paths {
        match tokio::fs::remove_file(&path).await {
            Ok(()) => deleted += 1,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }

    Ok(deleted)
}

fn artifact_filename(workflow_id: &str) -> String {
    let sanitized: String = workflow_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        "artifact".into()
    } else {
        sanitized
    }
}

async fn run_pipeline_process(
    pipeline: &Pipeline,
    pdf: Vec<u8>,
) -> Result<DocumentPipelineOutput, HandlerError> {
    match pipeline.process(&pdf).await {
        Ok(document) => Ok(document),
        Err(error) => {
            let disposition = pipeline.failure_disposition(PipelinePhase::Processing, &error);
            let message = error.to_string();
            match disposition {
                FailureDisposition::Retryable => Err(retryable_error(message)),
                FailureDisposition::Terminal => {
                    Err(TerminalError::new_with_code(TERMINAL_PROCESSING_CODE, message).into())
                }
            }
        }
    }
}

async fn stage_failure(
    ctx: &WorkflowContext<'_>,
    store: PostgresReviewStore,
    failure: FailureRecord,
) -> Result<(), TerminalError> {
    ctx.run(|| async move { store.stage(failure).await.map_err(retryable_error) })
        .name("stage-review-case")
        .await
}

fn retryable_error(error: impl std::fmt::Display) -> HandlerError {
    std::io::Error::other(error.to_string()).into()
}

#[cfg(test)]
mod tests {
    use super::{artifact_filename, delete_debug_artifacts};

    #[test]
    fn artifact_filenames_cannot_escape_the_debug_directories() {
        assert_eq!(artifact_filename("2AEJBJL6"), "2AEJBJL6");
        assert_eq!(artifact_filename("../../paper/one"), ".._.._paper_one");
        assert_eq!(artifact_filename(".."), "artifact");
    }

    #[tokio::test]
    async fn deleting_debug_artifacts_is_safe_and_idempotent() {
        let root = std::env::temp_dir().join(format!(
            "scepa-delete-artifacts-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::create_dir_all(root.join("tei")).await.unwrap();
        tokio::fs::create_dir_all(root.join("json")).await.unwrap();
        tokio::fs::write(root.join("tei/paper_one.tei.xml"), b"tei")
            .await
            .unwrap();
        tokio::fs::write(root.join("json/paper_one.json"), b"json")
            .await
            .unwrap();

        assert_eq!(delete_debug_artifacts(&root, "paper/one").await.unwrap(), 2);
        assert_eq!(delete_debug_artifacts(&root, "paper/one").await.unwrap(), 0);
        assert!(!root.join("tei/paper_one.tei.xml").exists());
        assert!(!root.join("json/paper_one.json").exists());

        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
