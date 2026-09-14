use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::search::SearchError;

#[derive(Clone)]
pub struct EmbeddingClient {
    client: reqwest::Client,
    host: String,
    api_key: String,
    model: String,
}

#[derive(Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: [&'a str; 1],
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    index: usize,
}

impl EmbeddingClient {
    pub fn new(host: String, api_key: String, model: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            host: host.trim_end_matches('/').to_owned(),
            api_key,
            model,
        }
    }

    pub async fn embed_query(&self, query: &str) -> Result<Vec<f32>, SearchError> {
        let response = self
            .client
            .post(format!("{}/embeddings", self.host))
            .bearer_auth(&self.api_key)
            .json(&EmbeddingRequest {
                model: &self.model,
                input: [query],
            })
            .send()
            .await
            .map_err(embedding_error)?;
        let status = response.status();
        if status != StatusCode::OK {
            let body = response.text().await.unwrap_or_default();
            return Err(SearchError::Embedding(format!(
                "embedding endpoint returned {status}: {body}"
            )));
        }
        let mut data = response
            .json::<EmbeddingResponse>()
            .await
            .map_err(embedding_error)?
            .data;
        if data.len() != 1 || data[0].index != 0 {
            return Err(SearchError::Embedding(
                "embedding endpoint did not return exactly index 0".into(),
            ));
        }
        Ok(data.remove(0).embedding)
    }
}

fn embedding_error(error: impl std::fmt::Display) -> SearchError {
    SearchError::Embedding(error.to_string())
}
