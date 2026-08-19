//! Qdrant collection lifecycle and passage point persistence.

use std::sync::Arc;

use qdrant_client::{
    Payload, Qdrant,
    qdrant::{
        CreateCollectionBuilder, DeletePointsBuilder, Distance, PointStruct, PointsIdsList,
        UpsertPointsBuilder, VectorParamsBuilder, vectors_config,
    },
};
use serde_json::json;
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QdrantConfig {
    pub url: String,
    pub collection: String,
    pub vector_dimension: u64,
    pub api_key: String,
}

impl QdrantConfig {
    pub fn new(
        url: impl Into<String>,
        collection: impl Into<String>,
        vector_dimension: u64,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into(),
            collection: collection.into(),
            vector_dimension,
            api_key: api_key.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum QdrantStoreError {
    #[error("failed to build the Qdrant client")]
    Build(#[source] qdrant_client::QdrantError),
    #[error("Qdrant operation failed for collection `{collection}`")]
    Operation {
        collection: String,
        #[source]
        source: qdrant_client::QdrantError,
    },
    #[error("Qdrant collection `{collection}` did not return its configuration")]
    MissingConfiguration { collection: String },
    #[error(
        "Qdrant collection `{collection}` uses named vectors; a single unnamed vector is required"
    )]
    NamedVectors { collection: String },
    #[error(
        "Qdrant collection `{collection}` has vector size {actual} and distance {distance}; expected size {expected} and cosine distance"
    )]
    IncompatibleCollection {
        collection: String,
        expected: u64,
        actual: u64,
        distance: i32,
    },
    #[error("expected vector dimension {expected}, got {actual}")]
    VectorDimensionMismatch { expected: u64, actual: usize },
}

impl QdrantStoreError {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::MissingConfiguration { .. }
                | Self::NamedVectors { .. }
                | Self::IncompatibleCollection { .. }
                | Self::VectorDimensionMismatch { .. }
        )
    }
}

#[derive(Clone)]
pub struct QdrantStore {
    client: Arc<Qdrant>,
    collection: String,
    vector_dimension: u64,
}

impl QdrantStore {
    pub async fn connect(config: &QdrantConfig) -> Result<Self, QdrantStoreError> {
        let client = Qdrant::from_url(&config.url)
            .api_key((!config.api_key.is_empty()).then_some(config.api_key.as_str()))
            .build()
            .map_err(QdrantStoreError::Build)?;

        if client
            .collection_exists(&config.collection)
            .await
            .map_err(|source| operation_error(config, source))?
        {
            validate_collection(&client, config).await?;
        } else {
            client
                .create_collection(
                    CreateCollectionBuilder::new(&config.collection).vectors_config(
                        VectorParamsBuilder::new(config.vector_dimension, Distance::Cosine),
                    ),
                )
                .await
                .map_err(|source| operation_error(config, source))?;
        }

        Ok(Self {
            client: Arc::new(client),
            collection: config.collection.clone(),
            vector_dimension: config.vector_dimension,
        })
    }

    pub fn vector_dimension(&self) -> u64 {
        self.vector_dimension
    }

    pub async fn apply(
        &self,
        delete_ids: Vec<String>,
        points: Vec<PointStruct>,
    ) -> Result<(), QdrantStoreError> {
        if !delete_ids.is_empty() {
            self.client
                .delete_points(
                    DeletePointsBuilder::new(&self.collection)
                        .points(PointsIdsList {
                            ids: delete_ids.into_iter().map(Into::into).collect(),
                        })
                        .wait(true),
                )
                .await
                .map_err(|source| QdrantStoreError::Operation {
                    collection: self.collection.clone(),
                    source,
                })?;
        }
        if !points.is_empty() {
            self.client
                .upsert_points(UpsertPointsBuilder::new(&self.collection, points).wait(true))
                .await
                .map_err(|source| QdrantStoreError::Operation {
                    collection: self.collection.clone(),
                    source,
                })?;
        }
        Ok(())
    }
}

pub fn passage_payload(pdf_hash: &str, is_abstract: bool, id: &str) -> Payload {
    json!({
        "pdf_hash": pdf_hash,
        "is_abstract": is_abstract,
        "id": id,
    })
    .try_into()
    .expect("passage payload is always a JSON object")
}

async fn validate_collection(
    client: &Qdrant,
    config: &QdrantConfig,
) -> Result<(), QdrantStoreError> {
    let info = client
        .collection_info(&config.collection)
        .await
        .map_err(|source| operation_error(config, source))?;
    let vectors = info
        .result
        .and_then(|result| result.config)
        .and_then(|config| config.params)
        .and_then(|params| params.vectors_config)
        .and_then(|vectors| vectors.config)
        .ok_or_else(|| QdrantStoreError::MissingConfiguration {
            collection: config.collection.clone(),
        })?;
    let vectors_config::Config::Params(params) = vectors else {
        return Err(QdrantStoreError::NamedVectors {
            collection: config.collection.clone(),
        });
    };
    if params.size != config.vector_dimension || params.distance != Distance::Cosine as i32 {
        return Err(QdrantStoreError::IncompatibleCollection {
            collection: config.collection.clone(),
            expected: config.vector_dimension,
            actual: params.size,
            distance: params.distance,
        });
    }
    Ok(())
}

fn operation_error(config: &QdrantConfig, source: qdrant_client::QdrantError) -> QdrantStoreError {
    QdrantStoreError::Operation {
        collection: config.collection.clone(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_contains_only_the_stable_passage_metadata() {
        let value: serde_json::Value = passage_payload("abc", true, "abstract_1").into();
        assert_eq!(
            value,
            json!({"pdf_hash": "abc", "is_abstract": true, "id": "abstract_1"})
        );
    }
}
