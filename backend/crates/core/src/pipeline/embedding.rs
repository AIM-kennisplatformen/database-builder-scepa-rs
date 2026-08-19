//! OpenAI-compatible embedding client with bounded process-wide concurrency.

use std::{sync::Arc, time::Duration};

use reqwest::{Response, StatusCode, header::RETRY_AFTER};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Semaphore;

const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);
const MAX_RETRY_ATTEMPTS: u32 = 5;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbeddingConfig {
    pub host: String,
    pub api_key: String,
    pub model: String,
    pub max_concurrency: usize,
}

impl EmbeddingConfig {
    pub fn new(
        host: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        max_concurrency: usize,
    ) -> Result<Self, EmbeddingError> {
        if max_concurrency == 0 {
            return Err(EmbeddingError::InvalidConcurrency);
        }
        let mut host = host.into();
        if !host.ends_with('/') {
            host.push('/');
        }
        Ok(Self {
            host,
            api_key: api_key.into(),
            model: model.into(),
            max_concurrency,
        })
    }
}

#[derive(Clone, Debug)]
pub struct EmbeddingSource {
    config: EmbeddingConfig,
    client: reqwest::Client,
    permits: Arc<Semaphore>,
}

#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("embedding concurrency must be greater than zero")]
    InvalidConcurrency,
    #[error("failed to call the embedding endpoint")]
    Request(#[from] reqwest::Error),
    #[error("embedding endpoint rejected the request ({status}): {body}")]
    UnsuccessfulResponse { status: StatusCode, body: String },
    #[error("embedding endpoint is rate limiting requests: {body}")]
    RateLimited {
        body: String,
        retry_after: Option<Duration>,
    },
    #[error("failed to decode the embedding response")]
    Decode(#[source] serde_json::Error),
    #[error("expected {expected} embeddings, got {actual}")]
    ResponseCountMismatch { expected: usize, actual: usize },
    #[error("expected embedding response index {expected}, got {actual}")]
    InvalidResponseIndex { expected: usize, actual: usize },
    #[error("embedding concurrency limiter was closed")]
    ConcurrencyClosed,
}

impl EmbeddingError {
    pub fn is_terminal(&self) -> bool {
        match self {
            Self::InvalidConcurrency
            | Self::Decode(_)
            | Self::ResponseCountMismatch { .. }
            | Self::InvalidResponseIndex { .. }
            | Self::ConcurrencyClosed => true,
            Self::UnsuccessfulResponse { status, .. } => status.is_client_error(),
            Self::Request(_) | Self::RateLimited { .. } => false,
        }
    }
}

#[derive(Serialize)]
struct EmbeddingsRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Deserialize)]
struct EmbeddingsResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    index: usize,
}

impl EmbeddingSource {
    pub fn new(config: EmbeddingConfig) -> Self {
        let permits = Arc::new(Semaphore::new(config.max_concurrency));
        Self {
            config,
            client: reqwest::Client::new(),
            permits,
        }
    }

    pub async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let mut attempt = 1;
        let mut retry_delay = INITIAL_RETRY_DELAY;
        loop {
            match self.embed_once(inputs).await {
                Ok(vectors) => return Ok(vectors),
                Err(EmbeddingError::RateLimited { retry_after, .. })
                    if attempt < MAX_RETRY_ATTEMPTS =>
                {
                    // embed_once releases its permit before backoff so unrelated
                    // workflows can continue using the shared request budget.
                    tokio::time::sleep(retry_after.unwrap_or(retry_delay)).await;
                    attempt += 1;
                    retry_delay = (retry_delay * 2).min(MAX_RETRY_DELAY);
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn embed_once(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let endpoint = format!("{}embeddings", self.config.host);
        let permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| EmbeddingError::ConcurrencyClosed)?;
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(&self.config.api_key)
            .json(&EmbeddingsRequest {
                model: &self.config.model,
                input: inputs,
            })
            .send()
            .await;
        let response = response?;
        let status = response.status();

        if status == StatusCode::TOO_MANY_REQUESTS {
            let retry_after = retry_after_header(&response);
            let body = response.text().await.unwrap_or_default();
            drop(permit);
            return Err(EmbeddingError::RateLimited {
                retry_after: retry_after.or_else(|| retry_after_from_body(&body)),
                body,
            });
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            drop(permit);
            return Err(EmbeddingError::UnsuccessfulResponse { status, body });
        }

        let bytes = response.bytes().await?;
        drop(permit);
        let body: EmbeddingsResponse =
            serde_json::from_slice(&bytes).map_err(EmbeddingError::Decode)?;
        ordered_vectors(body, inputs.len())
    }
}

fn ordered_vectors(
    mut body: EmbeddingsResponse,
    expected_count: usize,
) -> Result<Vec<Vec<f32>>, EmbeddingError> {
    if body.data.len() != expected_count {
        return Err(EmbeddingError::ResponseCountMismatch {
            expected: expected_count,
            actual: body.data.len(),
        });
    }
    body.data.sort_by_key(|item| item.index);
    if let Some((expected, item)) = body
        .data
        .iter()
        .enumerate()
        .find(|(expected, item)| item.index != *expected)
    {
        return Err(EmbeddingError::InvalidResponseIndex {
            expected,
            actual: item.index,
        });
    }
    Ok(body.data.into_iter().map(|item| item.embedding).collect())
}

fn retry_after_header(response: &Response) -> Option<Duration> {
    response
        .headers()
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

fn retry_after_from_body(body: &str) -> Option<Duration> {
    let start = body.find("after ")? + "after ".len();
    let rest = &body[start..];
    let end = rest
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(rest.len());
    (end > 0)
        .then(|| rest[..end].parse::<u64>().ok().map(Duration::from_secs))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn configuration_normalizes_host_and_rejects_zero_concurrency() {
        let config = EmbeddingConfig::new("https://example.test/v1", "key", "model", 4).unwrap();
        assert_eq!(config.host, "https://example.test/v1/");
        assert!(matches!(
            EmbeddingConfig::new("https://example.test/v1", "key", "model", 0),
            Err(EmbeddingError::InvalidConcurrency)
        ));
    }

    #[test]
    fn parses_provider_retry_hint() {
        assert_eq!(
            retry_after_from_body("Retry the request after 2 sec."),
            Some(Duration::from_secs(2))
        );
    }

    #[test]
    fn embedding_response_is_reordered_and_validated() {
        let body: EmbeddingsResponse = serde_json::from_value(serde_json::json!({
            "data": [
                {"embedding": [2.0], "index": 1},
                {"embedding": [1.0], "index": 0}
            ]
        }))
        .unwrap();
        assert_eq!(
            ordered_vectors(body, 2).unwrap(),
            vec![vec![1.0], vec![2.0]]
        );

        let duplicate: EmbeddingsResponse = serde_json::from_value(serde_json::json!({
            "data": [
                {"embedding": [1.0], "index": 0},
                {"embedding": [2.0], "index": 0}
            ]
        }))
        .unwrap();
        assert!(matches!(
            ordered_vectors(duplicate, 2),
            Err(EmbeddingError::InvalidResponseIndex { .. })
        ));
    }

    #[tokio::test]
    async fn empty_input_does_not_make_a_request() {
        let source = EmbeddingSource::new(
            EmbeddingConfig::new("http://localhost:0", "key", "model", 1).unwrap(),
        );
        assert!(source.embed(&[]).await.unwrap().is_empty());
    }

    #[test]
    fn cloned_clients_share_the_process_limiter() {
        let source = EmbeddingSource::new(
            EmbeddingConfig::new("http://localhost:0", "key", "model", 3).unwrap(),
        );
        let clone = source.clone();
        assert!(Arc::ptr_eq(&source.permits, &clone.permits));
        assert_eq!(source.permits.available_permits(), 3);
    }

    #[tokio::test]
    async fn concurrent_attempts_do_not_exceed_the_shared_limit() {
        let source = EmbeddingSource::new(
            EmbeddingConfig::new("http://localhost:0", "key", "model", 2).unwrap(),
        );
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));

        let mut calls = Vec::new();
        for _ in 0..6 {
            let permits = source.permits.clone();
            let active = active.clone();
            let maximum = maximum.clone();
            calls.push(tokio::spawn(async move {
                let permit = permits.acquire_owned().await.unwrap();
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                maximum.fetch_max(current, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(10)).await;
                active.fetch_sub(1, Ordering::SeqCst);
                drop(permit);
            }));
        }
        for call in calls {
            call.await.unwrap();
        }
        assert_eq!(maximum.load(Ordering::SeqCst), 2);
        assert_eq!(source.permits.available_permits(), 2);
    }
}
