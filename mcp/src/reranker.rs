use std::sync::{Arc, Mutex};

use hf_hub::{Repo, RepoType, api::sync::Api};
use ort::{
    inputs,
    session::{Session, builder::GraphOptimizationLevel},
    value::Tensor,
};
use tokenizers::{
    EncodeInput, PaddingParams, PaddingStrategy, Tokenizer, TruncationParams, TruncationStrategy,
};

use crate::search::{PassageReranker, SearchError};

const MAX_SEQUENCE_LENGTH: usize = 512;

#[derive(Clone)]
pub struct OnnxReranker {
    inner: Arc<Mutex<RerankerModel>>,
    batch_size: usize,
}

struct RerankerModel {
    tokenizer: Tokenizer,
    session: Session,
}

impl OnnxReranker {
    pub async fn load(
        model_id: String,
        revision: String,
        batch_size: usize,
    ) -> Result<Self, SearchError> {
        if batch_size == 0 {
            return Err(SearchError::Rerank(
                "RERANK_BATCH_SIZE must be greater than zero".into(),
            ));
        }
        let model = tokio::task::spawn_blocking(move || {
            let api = Api::new().map_err(rerank_error)?;
            let repository = api.repo(Repo::with_revision(model_id, RepoType::Model, revision));
            let model_path = repository.get("onnx/model.onnx").map_err(rerank_error)?;
            let tokenizer_path = repository.get("tokenizer.json").map_err(rerank_error)?;
            let mut tokenizer = Tokenizer::from_file(tokenizer_path).map_err(rerank_error)?;
            tokenizer
                .with_truncation(Some(TruncationParams {
                    max_length: MAX_SEQUENCE_LENGTH,
                    strategy: TruncationStrategy::LongestFirst,
                    ..Default::default()
                }))
                .map_err(rerank_error)?;
            tokenizer.with_padding(Some(PaddingParams {
                strategy: PaddingStrategy::BatchLongest,
                ..Default::default()
            }));
            let session = Session::builder()
                .map_err(rerank_error)?
                .with_optimization_level(GraphOptimizationLevel::Level3)
                .map_err(rerank_error)?
                .commit_from_file(model_path)
                .map_err(rerank_error)?;
            Ok::<_, SearchError>(RerankerModel { tokenizer, session })
        })
        .await
        .map_err(rerank_error)??;
        Ok(Self {
            inner: Arc::new(Mutex::new(model)),
            batch_size,
        })
    }
}

#[async_trait::async_trait]
impl PassageReranker for OnnxReranker {
    async fn rerank(&self, query: &str, passages: &[String]) -> Result<Vec<f32>, SearchError> {
        let inner = self.inner.clone();
        let query = query.to_owned();
        let passages = passages.to_vec();
        let batch_size = self.batch_size;
        tokio::task::spawn_blocking(move || {
            let mut model = inner
                .lock()
                .map_err(|_| SearchError::Rerank("reranker lock was poisoned".into()))?;
            let mut scores = Vec::with_capacity(passages.len());
            for batch in passages.chunks(batch_size) {
                scores.extend(model.predict(&query, batch)?);
            }
            Ok(scores)
        })
        .await
        .map_err(rerank_error)?
    }
}

impl RerankerModel {
    fn predict(&mut self, query: &str, passages: &[String]) -> Result<Vec<f32>, SearchError> {
        let inputs: Vec<EncodeInput> = passages
            .iter()
            .map(|passage| (query.to_owned(), passage.clone()).into())
            .collect();
        let encodings = self
            .tokenizer
            .encode_batch(inputs, true)
            .map_err(rerank_error)?;
        let sequence_length = encodings
            .first()
            .map(|encoding| encoding.len())
            .unwrap_or_default();
        let shape = [encodings.len(), sequence_length];
        let input_ids = encodings
            .iter()
            .flat_map(|encoding| encoding.get_ids().iter().map(|value| i64::from(*value)))
            .collect::<Vec<_>>();
        let attention_mask = encodings
            .iter()
            .flat_map(|encoding| {
                encoding
                    .get_attention_mask()
                    .iter()
                    .map(|value| i64::from(*value))
            })
            .collect::<Vec<_>>();
        let token_type_ids = encodings
            .iter()
            .flat_map(|encoding| {
                encoding
                    .get_type_ids()
                    .iter()
                    .map(|value| i64::from(*value))
            })
            .collect::<Vec<_>>();
        let outputs = self
            .session
            .run(inputs![
                "input_ids" => Tensor::from_array((shape, input_ids)).map_err(rerank_error)?,
                "attention_mask" => Tensor::from_array((shape, attention_mask)).map_err(rerank_error)?,
                "token_type_ids" => Tensor::from_array((shape, token_type_ids)).map_err(rerank_error)?,
            ])
            .map_err(rerank_error)?;
        let (_, logits) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(rerank_error)?;
        Ok(logits.iter().map(|logit| sigmoid(*logit)).collect())
    }
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

fn rerank_error(error: impl std::fmt::Display) -> SearchError {
    SearchError::Rerank(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigmoid_maps_logits_to_probabilities() {
        assert!((sigmoid(0.0) - 0.5).abs() < f32::EPSILON);
        assert!(sigmoid(5.0) > sigmoid(-5.0));
    }
}
