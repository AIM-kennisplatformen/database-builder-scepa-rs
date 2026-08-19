//! Primitives for defining durable asynchronous pipeline services.
//!
//! A [`PipelineService`] consists of three phases:
//!
//! 1. input validation,
//! 2. processing,
//! 3. output validation.
//!
//! Each phase can be invoked independently. [`PipelineService::execute`] runs
//! the complete lifecycle in order. Terminal failures are staged in a
//! [`ReviewStore`] before the original error is propagated; retryable failures
//! are propagated directly to the durable orchestrator.
//!
//! Validation may also produce non-fatal, strongly typed warnings. Warnings
//! from input and output validation are accumulated and returned with the
//! successful [`PipelineOutcome`].
//!
//! # Failure staging
//!
//! A terminal pipeline phase failure is represented by a [`FailureRecord`]
//! containing the workflow ID, service, failed phase, propagated error, and an
//! application-defined [`ReviewArtifact`].
//!
//! [`ReviewStore::stage`] is part of the pipeline's durability boundary:
//! returning `Ok(())` means the failure is durably available for later review.
//! If staging itself fails, [`PipelineService::execute`] returns the staging
//! error rather than the original pipeline error.

pub mod document;
pub mod embedding;
pub mod garage;
pub mod grobid;
pub mod qdrant;
pub mod tei;
pub mod typedb;
pub mod vector;

pub use document::{DocumentPipelineOutput, DocumentPipelineService, DocumentPipelineWarning};

use std::error::Error;

use async_trait::async_trait;

/// Non-fatal findings produced by a validation phase.
///
/// Unlike validation errors, warnings do not prevent the pipeline from
/// continuing. The warning type is defined by the [`PipelineService`]
/// implementation so callers can inspect findings without relying on string
/// parsing or unstable message text.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[must_use = "validation warnings must be deliberately acknowledged"]
pub struct ValidationReport<W> {
    warnings: Vec<W>,
}

impl<W> ValidationReport<W> {
    /// Returns a validation report with no warnings.
    pub fn clean() -> Self {
        Self {
            warnings: Vec::new(),
        }
    }

    /// Returns a validation report containing a single warning.
    pub fn warning(warning: W) -> Self {
        Self {
            warnings: vec![warning],
        }
    }

    /// Returns a validation report containing the supplied warnings.
    pub fn warnings(warnings: impl IntoIterator<Item = W>) -> Self {
        Self {
            warnings: warnings.into_iter().collect(),
        }
    }

    /// Returns the warnings contained in this report.
    pub fn as_slice(&self) -> &[W] {
        &self.warnings
    }

    /// Appends all warnings from `other` to this report.
    fn extend(&mut self, other: Self) {
        self.warnings.extend(other.warnings);
    }
}

/// The successful result of a pipeline execution.
///
/// Callers can either acknowledge warnings through [`PipelineOutcome::into_output`]
/// or retain them alongside the output through [`PipelineOutcome::into_parts`].
#[derive(Clone, Debug, PartialEq)]
#[must_use = "call into_output to acknowledge warnings or into_parts to retain them"]
pub struct PipelineOutcome<T, W> {
    output: T,
    warnings: Vec<W>,
}

impl<T, W> PipelineOutcome<T, W> {
    /// Splits the outcome into its wire-friendly output and warning values.
    pub fn into_parts(self) -> (T, Vec<W>) {
        (self.output, self.warnings)
    }

    /// Acknowledges validation warnings and returns the pipeline output.
    ///
    /// `handle_warnings` is invoked exactly once, including when no warnings
    /// were produced.
    pub fn into_output(self, handle_warnings: impl FnOnce(&[W])) -> T {
        handle_warnings(&self.warnings);
        self.output
    }

    /// Returns the validation warnings without consuming the outcome.
    pub fn warnings(&self) -> &[W] {
        &self.warnings
    }
}

/// A pipeline failure together with the retry decision made by the service.
///
/// Durable adapters use this type to preserve [`PipelineService`]'s failure
/// classification when translating an error into an orchestrator-specific
/// error type.
#[derive(Debug)]
pub struct PipelineExecutionError {
    disposition: FailureDisposition,
    source: eros::ErrorUnion,
}

impl PipelineExecutionError {
    /// Whether a durable orchestrator should retry the failed execution.
    pub fn disposition(&self) -> FailureDisposition {
        self.disposition
    }

    /// Returns the underlying pipeline or failure-staging error.
    pub fn into_source(self) -> eros::ErrorUnion {
        self.source
    }
}

impl std::fmt::Display for PipelineExecutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(formatter)
    }
}

impl Error for PipelineExecutionError {}

/// The phase in which a pipeline service failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelinePhase {
    /// The input failed validation before processing started.
    InputValidation,

    /// The service failed while performing its primary operation.
    Processing,

    /// Processing succeeded, but the produced output failed validation.
    OutputValidation,
}

impl PipelinePhase {
    /// Stable database representation of this phase.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InputValidation => "input_validation",
            Self::Processing => "processing",
            Self::OutputValidation => "output_validation",
        }
    }
}

/// Whether a failed phase should be retried by the durable orchestrator or
/// handed to an operator for review.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureDisposition {
    /// Propagate the error without creating a review case.
    Retryable,

    /// Durably stage the failure before propagating the error.
    Terminal,
}

/// Evidence retained for operator review of a failed pipeline item.
///
/// The representation is intentionally generic. Implementations may store raw
/// data directly or encode a reference to immutable content stored elsewhere.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewArtifact {
    /// Media type of the artifact contents.
    pub content_type: String,

    /// Artifact payload or application-defined serialized reference.
    pub bytes: Vec<u8>,
}

/// A pipeline failure staged for later review.
///
/// This contains the portable information required by the pipeline contract.
/// Persistence implementations may associate additional metadata such as IDs,
/// timestamps, retry state, source records, or tenant information.
#[derive(Debug)]
pub struct FailureRecord {
    /// Stable identifier of the durable execution.
    ///
    /// Review stores use this with `service` and `phase` as an idempotency key.
    pub workflow_id: String,

    /// Stable identifier of the service that failed.
    pub service: &'static str,

    /// Phase in which the failure occurred.
    pub phase: PipelinePhase,

    /// Classification of the original failure before it was staged.
    pub disposition: FailureDisposition,

    /// Human-readable representation of the error propagated by the failed
    /// phase.
    ///
    /// The pipeline returns the original error to its caller. `ErrorUnion` is
    /// not generally cloneable, so staging retains this durable representation
    /// instead of taking ownership of the error that must be returned.
    pub error_message: String,

    /// Evidence associated with the failed item.
    pub artifact: ReviewArtifact,
}

/// Durable storage for pipeline failures requiring review.
///
/// [`ReviewStore::stage`] must not return `Ok(())` until the supplied record is
/// durably persisted. The storage mechanism itself is implementation-defined.
#[async_trait]
pub trait ReviewStore: Send + Sync {
    /// Durably stages a pipeline failure for later review.
    async fn stage(&self, failure: FailureRecord) -> eros::Result<()>;
}

/// A pipeline operation with validation before and after processing.
///
/// Implementations provide three independently callable phases:
///
/// - [`PipelineService::validate_input`]
/// - [`PipelineService::process`]
/// - [`PipelineService::validate_output`]
///
/// Callers may invoke those phases independently when implementing custom
/// orchestration. [`PipelineService::execute`] provides the standard lifecycle:
///
/// `validate_input -> process -> validate_output`
///
/// Failures are terminal by default and are staged as a [`FailureRecord`]
/// before the phase error is propagated. Services must explicitly classify
/// errors that a durable orchestrator may retry.
///
/// Successful input- and output-validation warnings are accumulated into the
/// returned [`PipelineOutcome`].
#[async_trait]
pub trait PipelineService: Send + Sync {
    /// Input accepted by this service.
    type Input: Send + Sync;

    /// Output produced by this service.
    type Output: Send + Sync;

    /// Strongly typed non-fatal validation finding.
    ///
    /// Warnings should represent conditions that are relevant to callers or
    /// operators but do not make the input or output invalid.
    type Warning: Error + Send + Sync + 'static;

    /// Stable identifier used in failure records and operational telemetry.
    const NAME: &'static str;

    /// Returns the store used to persist failures requiring review.
    fn review_store(&self) -> &dyn ReviewStore;

    /// Classifies a phase failure as retryable or terminal.
    ///
    /// All failures are terminal by default. Services should override this
    /// method only when they can positively identify a transient condition.
    fn failure_disposition(
        &self,
        _phase: PipelinePhase,
        _error: &eros::ErrorUnion,
    ) -> FailureDisposition {
        FailureDisposition::Terminal
    }

    /// Durably records input-derived data before validation or processing.
    ///
    /// Persistence failures are retryable so an orchestrator cannot continue
    /// until data that already exists has crossed its durability boundary.
    async fn persist_input(&self, _workflow_id: &str, _input: &Self::Input) -> eros::Result<()> {
        Ok(())
    }

    /// Validates `input` before processing.
    ///
    /// Returning `Err` prevents [`PipelineService::process`] from running.
    /// Returning `Ok` may contain non-fatal, strongly typed warnings.
    async fn validate_input(
        &self,
        input: &Self::Input,
    ) -> eros::Result<ValidationReport<Self::Warning>>;

    /// Performs the primary operation of this service.
    async fn process(&self, input: &Self::Input) -> eros::Result<Self::Output>;

    /// Validates an output produced by [`PipelineService::process`].
    ///
    /// Returning `Err` stages the produced output for review. Returning `Ok`
    /// may contain additional non-fatal, strongly typed warnings.
    async fn validate_output(
        &self,
        output: &Self::Output,
    ) -> eros::Result<ValidationReport<Self::Warning>>;

    /// Builds the evidence retained when this service fails.
    ///
    /// `output` is `Some` only when processing succeeded and output validation
    /// failed. For failures during input validation or processing, it is `None`.
    fn review_artifact(&self, input: &Self::Input, output: Option<&Self::Output>)
    -> ReviewArtifact;

    /// Executes the complete service lifecycle.
    ///
    /// The phases run in order:
    ///
    /// 1. [`PipelineService::validate_input`]
    /// 2. [`PipelineService::process`]
    /// 3. [`PipelineService::validate_output`]
    ///
    /// Validation warnings from both validation phases are accumulated in the
    /// returned [`PipelineOutcome`].
    ///
    /// # Failure behavior
    ///
    /// A retryable phase failure is returned directly in a
    /// [`PipelineExecutionError`]. A terminal failure is first passed to
    /// [`ReviewStore::stage`]. Once staging succeeds, the original phase error
    /// is retained as the source of a terminal [`PipelineExecutionError`].
    ///
    /// If staging fails, the staging error is retained as the source of a
    /// retryable [`PipelineExecutionError`] because durable handoff to the
    /// review system could not be confirmed.
    async fn execute(
        &self,
        workflow_id: &str,
        input: &Self::Input,
    ) -> Result<PipelineOutcome<Self::Output, Self::Warning>, PipelineExecutionError> {
        if let Err(error) = self.persist_input(workflow_id, input).await {
            return Err(PipelineExecutionError {
                disposition: FailureDisposition::Retryable,
                source: error,
            });
        }

        let mut report = match self.validate_input(input).await {
            Ok(report) => report,

            Err(error) => {
                return self
                    .classify_failure(
                        workflow_id,
                        input,
                        None,
                        PipelinePhase::InputValidation,
                        error,
                    )
                    .await;
            }
        };

        let output = match self.process(input).await {
            Ok(output) => output,

            Err(error) => {
                return self
                    .classify_failure(workflow_id, input, None, PipelinePhase::Processing, error)
                    .await;
            }
        };

        match self.validate_output(&output).await {
            Ok(post_report) => {
                report.extend(post_report);
            }

            Err(error) => {
                return self
                    .classify_failure(
                        workflow_id,
                        input,
                        Some(&output),
                        PipelinePhase::OutputValidation,
                        error,
                    )
                    .await;
            }
        }

        Ok(PipelineOutcome {
            output,
            warnings: report.warnings,
        })
    }

    /// Stages terminal failures and retains the resulting retry decision.
    async fn classify_failure<T>(
        &self,
        workflow_id: &str,
        input: &Self::Input,
        output: Option<&Self::Output>,
        phase: PipelinePhase,
        error: eros::ErrorUnion,
    ) -> Result<T, PipelineExecutionError>
    where
        T: Send,
    {
        if self.failure_disposition(phase, &error) == FailureDisposition::Retryable {
            return Err(PipelineExecutionError {
                disposition: FailureDisposition::Retryable,
                source: error,
            });
        }

        match self
            .stage_failure(workflow_id, input, output, phase, &error)
            .await
        {
            Ok(()) => Err(PipelineExecutionError {
                disposition: FailureDisposition::Terminal,
                source: error,
            }),
            Err(staging_error) => Err(PipelineExecutionError {
                disposition: FailureDisposition::Retryable,
                source: staging_error,
            }),
        }
    }

    /// Persists the failure record used by both execution entry points.
    async fn stage_failure(
        &self,
        workflow_id: &str,
        input: &Self::Input,
        output: Option<&Self::Output>,
        phase: PipelinePhase,
        error: &eros::ErrorUnion,
    ) -> eros::Result<()> {
        let artifact = self.review_artifact(input, output);

        let error_message = error.to_string();
        let disposition = self.failure_disposition(phase, error);

        self.review_store()
            .stage(FailureRecord {
                workflow_id: workflow_id.to_owned(),
                service: Self::NAME,
                phase,
                disposition,
                error_message,
                artifact,
            })
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        error::Error,
        fmt,
        sync::{Arc, Mutex},
    };

    use super::*;

    #[derive(Clone, Default)]
    struct RecordingStore {
        failures: Arc<Mutex<Vec<FailureRecord>>>,
    }

    #[async_trait]
    impl ReviewStore for RecordingStore {
        async fn stage(&self, failure: FailureRecord) -> eros::Result<()> {
            self.failures.lock().unwrap().push(failure);
            Ok(())
        }
    }

    #[derive(Clone, Copy)]
    enum FailingPhase {
        Input,
        Processing,
        Output,
    }

    #[derive(Debug)]
    struct TestWarning;

    impl fmt::Display for TestWarning {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("test warning")
        }
    }

    impl Error for TestWarning {}

    struct TestService {
        failing_phase: FailingPhase,
        processing_is_retryable: bool,
        review_store: RecordingStore,
    }

    #[async_trait]
    impl PipelineService for TestService {
        type Input = String;
        type Output = String;
        type Warning = TestWarning;

        const NAME: &'static str = "test-service";

        fn review_store(&self) -> &dyn ReviewStore {
            &self.review_store
        }

        fn failure_disposition(
            &self,
            phase: PipelinePhase,
            _error: &eros::ErrorUnion,
        ) -> FailureDisposition {
            if phase == PipelinePhase::Processing && self.processing_is_retryable {
                FailureDisposition::Retryable
            } else {
                FailureDisposition::Terminal
            }
        }

        async fn validate_input(
            &self,
            _input: &Self::Input,
        ) -> eros::Result<ValidationReport<Self::Warning>> {
            if matches!(self.failing_phase, FailingPhase::Input) {
                eros::bail!("input failed")
            }
            Ok(ValidationReport::clean())
        }

        async fn process(&self, input: &Self::Input) -> eros::Result<Self::Output> {
            if matches!(self.failing_phase, FailingPhase::Processing) {
                eros::bail!("processing failed")
            }
            Ok(input.clone())
        }

        async fn validate_output(
            &self,
            _output: &Self::Output,
        ) -> eros::Result<ValidationReport<Self::Warning>> {
            if matches!(self.failing_phase, FailingPhase::Output) {
                eros::bail!("output failed")
            }
            Ok(ValidationReport::clean())
        }

        fn review_artifact(
            &self,
            input: &Self::Input,
            _output: Option<&Self::Output>,
        ) -> ReviewArtifact {
            ReviewArtifact {
                content_type: "text/plain".into(),
                bytes: input.as_bytes().to_vec(),
            }
        }
    }

    #[tokio::test]
    async fn processing_failures_are_left_for_the_orchestrator_to_retry() {
        let store = RecordingStore::default();
        let service = TestService {
            failing_phase: FailingPhase::Processing,
            processing_is_retryable: true,
            review_store: store.clone(),
        };

        assert!(
            service
                .execute("workflow-1", &"input".into())
                .await
                .is_err()
        );
        assert!(store.failures.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn classified_execution_preserves_retryable_failures() {
        let store = RecordingStore::default();
        let service = TestService {
            failing_phase: FailingPhase::Processing,
            processing_is_retryable: true,
            review_store: store.clone(),
        };

        let error = service
            .execute("workflow-1", &"input".into())
            .await
            .unwrap_err();

        assert_eq!(error.disposition(), FailureDisposition::Retryable);
        assert!(store.failures.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn processing_failures_are_terminal_by_default() {
        let store = RecordingStore::default();
        let service = TestService {
            failing_phase: FailingPhase::Processing,
            processing_is_retryable: false,
            review_store: store.clone(),
        };

        assert!(
            service
                .execute("workflow-1", &"input".into())
                .await
                .is_err()
        );
        assert_eq!(store.failures.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn classified_execution_preserves_staged_terminal_failures() {
        let store = RecordingStore::default();
        let service = TestService {
            failing_phase: FailingPhase::Processing,
            processing_is_retryable: false,
            review_store: store.clone(),
        };

        let error = service
            .execute("workflow-1", &"input".into())
            .await
            .unwrap_err();

        assert_eq!(error.disposition(), FailureDisposition::Terminal);
        assert_eq!(store.failures.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn input_validation_failures_are_staged_with_the_workflow_id() {
        let store = RecordingStore::default();
        let service = TestService {
            failing_phase: FailingPhase::Input,
            processing_is_retryable: false,
            review_store: store.clone(),
        };

        assert!(
            service
                .execute("workflow-1", &"input".into())
                .await
                .is_err()
        );

        let failures = store.failures.lock().unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].workflow_id, "workflow-1");
        assert_eq!(failures[0].phase, PipelinePhase::InputValidation);
    }

    #[tokio::test]
    async fn output_validation_failures_are_staged() {
        let store = RecordingStore::default();
        let service = TestService {
            failing_phase: FailingPhase::Output,
            processing_is_retryable: false,
            review_store: store.clone(),
        };

        assert!(
            service
                .execute("workflow-1", &"input".into())
                .await
                .is_err()
        );
        assert_eq!(store.failures.lock().unwrap().len(), 1);
    }
}
