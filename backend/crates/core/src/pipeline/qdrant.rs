//! Qdrant collection lifecycle and passage point persistence.

use std::sync::Arc;

use qdrant_client::{
    Payload, Qdrant,
    qdrant::{
        CreateCollectionBuilder, CreateFieldIndexCollectionBuilder, DeletePointsBuilder, Distance,
        FieldType, PayloadSchemaInfo, PayloadSchemaType, PointStruct, PointsIdsList,
        UpsertPointsBuilder, VectorParamsBuilder, vectors_config,
    },
};
use serde::Serialize;
use serde_json::json;
use thiserror::Error;

use crate::models::draft::BoundingBox;

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
    #[error(
        "Qdrant collection `{collection}` has payload index `{field}` with type {actual}; expected {expected}"
    )]
    IncompatiblePayloadIndex {
        collection: String,
        field: &'static str,
        expected: &'static str,
        actual: i32,
    },
}

impl QdrantStoreError {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::MissingConfiguration { .. }
                | Self::NamedVectors { .. }
                | Self::IncompatibleCollection { .. }
                | Self::VectorDimensionMismatch { .. }
                | Self::IncompatiblePayloadIndex { .. }
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
        ensure_payload_indexes(&client, config).await?;

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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SourcePointPayloadData {
    pub pdf_hash: String,
    pub is_abstract: bool,
    pub is_combined: bool,
    pub id: String,
    pub text: String,
    pub combined_point_ids: Vec<String>,
    pub bounding_boxes: Vec<BoundingBox>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CombinedPointPayloadData {
    pub pdf_hash: String,
    pub is_abstract: bool,
    pub is_combined: bool,
    pub id: String,
    pub text: String,
    pub source_point_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum PointPayloadData {
    Source(SourcePointPayloadData),
    Combined(CombinedPointPayloadData),
}

pub fn point_payload(data: &PointPayloadData) -> Payload {
    json!(data)
        .try_into()
        .expect("passage payload is always a JSON object")
}

const REQUIRED_PAYLOAD_INDEXES: [(&str, FieldType, PayloadSchemaType); 3] = [
    ("is_abstract", FieldType::Bool, PayloadSchemaType::Bool),
    ("is_combined", FieldType::Bool, PayloadSchemaType::Bool),
    ("pdf_hash", FieldType::Keyword, PayloadSchemaType::Keyword),
];

async fn ensure_payload_indexes(
    client: &Qdrant,
    config: &QdrantConfig,
) -> Result<(), QdrantStoreError> {
    let info = client
        .collection_info(&config.collection)
        .await
        .map_err(|source| operation_error(config, source))?
        .result
        .ok_or_else(|| QdrantStoreError::MissingConfiguration {
            collection: config.collection.clone(),
        })?;
    for (field, field_type) in missing_payload_indexes(config, &info.payload_schema)? {
        client
            .create_field_index(
                CreateFieldIndexCollectionBuilder::new(&config.collection, field, field_type)
                    .wait(true),
            )
            .await
            .map_err(|source| operation_error(config, source))?;
    }
    Ok(())
}

fn missing_payload_indexes(
    config: &QdrantConfig,
    schema: &std::collections::HashMap<String, PayloadSchemaInfo>,
) -> Result<Vec<(&'static str, FieldType)>, QdrantStoreError> {
    let mut missing = Vec::new();
    for (field, field_type, schema_type) in REQUIRED_PAYLOAD_INDEXES {
        match schema.get(field) {
            Some(info) if info.data_type == schema_type as i32 => {}
            Some(info) => {
                return Err(QdrantStoreError::IncompatiblePayloadIndex {
                    collection: config.collection.clone(),
                    field,
                    expected: schema_type.as_str_name(),
                    actual: info.data_type,
                });
            }
            None => missing.push((field, field_type)),
        }
    }
    Ok(missing)
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
    fn source_payload_contains_only_source_fields() {
        let coordinates = vec![BoundingBox {
            page: Some(2),
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        }];
        let value: serde_json::Value =
            point_payload(&PointPayloadData::Source(SourcePointPayloadData {
                pdf_hash: "abc".into(),
                is_abstract: true,
                is_combined: false,
                id: "abstract_1".into(),
                text: "An abstract.".into(),
                combined_point_ids: vec!["d85e465b-6b5e-51f0-a818-3c2b315b48c6".into()],
                bounding_boxes: coordinates.clone(),
                section: None,
                heading: None,
            }))
            .into();
        assert_eq!(
            value,
            json!({
                "pdf_hash": "abc",
                "is_abstract": true,
                "is_combined": false,
                "id": "abstract_1",
                "text": "An abstract.",
                "combined_point_ids": ["d85e465b-6b5e-51f0-a818-3c2b315b48c6"],
                "bounding_boxes": [{
                    "page": 2,
                    "x": 10.0,
                    "y": 20.0,
                    "width": 30.0,
                    "height": 40.0
                }]
            })
        );
    }

    #[test]
    fn combined_payload_contains_only_combined_fields() {
        let value: serde_json::Value =
            point_payload(&PointPayloadData::Combined(CombinedPointPayloadData {
                pdf_hash: "abc".into(),
                is_abstract: false,
                is_combined: true,
                id: "combined_body_00000001".into(),
                text: "Combined text.".into(),
                source_point_ids: vec!["e2c35ec9-3afb-5c31-a1c6-e6ea70bc61f3".into()],
                section: Some("Methods".into()),
                heading: None,
            }))
            .into();
        assert_eq!(
            value,
            json!({
                "pdf_hash": "abc",
                "is_abstract": false,
                "is_combined": true,
                "id": "combined_body_00000001",
                "text": "Combined text.",
                "source_point_ids": ["e2c35ec9-3afb-5c31-a1c6-e6ea70bc61f3"],
                "section": "Methods"
            })
        );
    }

    fn schema_info(data_type: PayloadSchemaType) -> PayloadSchemaInfo {
        PayloadSchemaInfo {
            data_type: data_type as i32,
            params: None,
            points: None,
        }
    }

    #[test]
    fn payload_index_reconciliation_finds_missing_indexes() {
        let config = QdrantConfig::new("url", "collection", 3, "");
        let schema = std::collections::HashMap::from([(
            "is_abstract".into(),
            schema_info(PayloadSchemaType::Bool),
        )]);
        assert_eq!(
            missing_payload_indexes(&config, &schema).unwrap(),
            [
                ("is_combined", FieldType::Bool),
                ("pdf_hash", FieldType::Keyword)
            ]
        );
    }

    #[test]
    fn payload_index_reconciliation_accepts_required_indexes() {
        let config = QdrantConfig::new("url", "collection", 3, "");
        let schema = std::collections::HashMap::from([
            ("is_abstract".into(), schema_info(PayloadSchemaType::Bool)),
            ("is_combined".into(), schema_info(PayloadSchemaType::Bool)),
            ("pdf_hash".into(), schema_info(PayloadSchemaType::Keyword)),
        ]);
        assert!(
            missing_payload_indexes(&config, &schema)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn payload_index_reconciliation_rejects_wrong_types() {
        let config = QdrantConfig::new("url", "collection", 3, "");
        let schema = std::collections::HashMap::from([(
            "is_abstract".into(),
            schema_info(PayloadSchemaType::Keyword),
        )]);
        assert!(matches!(
            missing_payload_indexes(&config, &schema),
            Err(QdrantStoreError::IncompatiblePayloadIndex {
                field: "is_abstract",
                ..
            })
        ));
    }
}
