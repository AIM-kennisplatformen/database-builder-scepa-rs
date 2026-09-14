use std::collections::BTreeSet;

use qdrant_client::{
    Qdrant,
    qdrant::{Condition, Filter, GetPointsBuilder, QueryPointsBuilder, point_id::PointIdOptions},
};
use serde::Deserialize;

use crate::{models::CombinedPassageCandidate, search::SearchError};

#[derive(Clone)]
pub struct PassageStore {
    client: Qdrant,
    collection: String,
    vector_size: u64,
}

#[derive(Deserialize)]
struct SourcePayload {
    combined_point_ids: Vec<String>,
}

#[derive(Deserialize)]
struct CombinedPayload {
    pdf_hash: String,
    text: String,
    is_combined: bool,
}

impl PassageStore {
    pub async fn connect(
        url: &str,
        collection: String,
        vector_size: u64,
        api_key: &str,
    ) -> Result<Self, SearchError> {
        let client = Qdrant::from_url(url)
            .api_key((!api_key.is_empty()).then_some(api_key))
            .build()
            .map_err(qdrant_error)?;
        if !client
            .collection_exists(&collection)
            .await
            .map_err(qdrant_error)?
        {
            return Err(SearchError::Qdrant(format!(
                "collection `{collection}` does not exist"
            )));
        }
        Ok(Self {
            client,
            collection,
            vector_size,
        })
    }

    pub async fn combined_candidates(
        &self,
        query_vector: Vec<f32>,
        eligible_pdf_hashes: &[String],
        source_limit: usize,
    ) -> Result<Vec<CombinedPassageCandidate>, SearchError> {
        if query_vector.len() as u64 != self.vector_size {
            return Err(SearchError::Qdrant(format!(
                "query embedding has dimension {}, expected {}",
                query_vector.len(),
                self.vector_size
            )));
        }
        let response = self
            .client
            .query(
                QueryPointsBuilder::new(&self.collection)
                    .query(query_vector)
                    .limit(source_limit as u64)
                    .filter(Filter::all([
                        Condition::matches("is_combined", false),
                        Condition::matches("pdf_hash", eligible_pdf_hashes.to_vec()),
                    ]))
                    .with_payload(true),
            )
            .await
            .map_err(qdrant_error)?;
        let mut combined_ids = BTreeSet::new();
        for point in response.result {
            let payload: SourcePayload = decode_payload(point.payload, "source")?;
            combined_ids.extend(payload.combined_point_ids);
        }
        if combined_ids.is_empty() {
            return Ok(Vec::new());
        }
        let eligible_pdf_hashes = eligible_pdf_hashes
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();

        let response = self
            .client
            .get_points(
                GetPointsBuilder::new(
                    &self.collection,
                    combined_ids.into_iter().map(Into::into).collect::<Vec<_>>(),
                )
                .with_payload(true)
                .with_vectors(false),
            )
            .await
            .map_err(qdrant_error)?;
        let candidates = response
            .result
            .into_iter()
            .map(|point| {
                let point_id = point
                    .id
                    .as_ref()
                    .map(|id| match id.point_id_options.as_ref() {
                        Some(PointIdOptions::Uuid(value)) => value.clone(),
                        Some(PointIdOptions::Num(value)) => value.to_string(),
                        None => String::new(),
                    })
                    .unwrap_or_default();
                let payload: CombinedPayload = decode_payload(point.payload, "combined")?;
                if !payload.is_combined {
                    return Err(SearchError::Qdrant(format!(
                        "point {point_id} linked as combined has is_combined=false"
                    )));
                }
                Ok(CombinedPassageCandidate {
                    point_id,
                    pdf_hash: payload.pdf_hash,
                    text: payload.text,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(candidates
            .into_iter()
            .filter(|candidate| eligible_pdf_hashes.contains(candidate.pdf_hash.as_str()))
            .collect())
    }
}

fn decode_payload<T: for<'de> Deserialize<'de>>(
    payload: std::collections::HashMap<String, qdrant_client::qdrant::Value>,
    kind: &str,
) -> Result<T, SearchError> {
    let value = serde_json::Value::Object(
        payload
            .into_iter()
            .map(|(key, value)| (key, value.into_json()))
            .collect(),
    );
    serde_json::from_value(value)
        .map_err(|error| SearchError::Qdrant(format!("invalid {kind} payload: {error}")))
}

fn qdrant_error(error: impl std::fmt::Display) -> SearchError {
    SearchError::Qdrant(error.to_string())
}
