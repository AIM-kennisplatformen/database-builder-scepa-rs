use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use thiserror::Error;

use crate::{
    embedding::EmbeddingClient,
    models::{LiteratureFilters, LiteratureResult, LiteratureSearchResponse, MetadataResponse},
    qdrant::PassageStore,
    reranker::OnnxReranker,
    typedb::{MetadataStore, validate_filters},
};

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("invalid search request: {0}")]
    InvalidInput(String),
    #[error("embedding failed: {0}")]
    Embedding(String),
    #[error("TypeDB retrieval failed: {0}")]
    TypeDb(String),
    #[error("Qdrant retrieval failed: {0}")]
    Qdrant(String),
    #[error("reranking failed: {0}")]
    Rerank(String),
}

#[async_trait]
pub trait PassageReranker: Send + Sync {
    async fn rerank(&self, query: &str, passages: &[String]) -> Result<Vec<f32>, SearchError>;
}

#[derive(Clone)]
pub struct LiteratureSearchService {
    metadata: MetadataStore,
    passages: PassageStore,
    embeddings: EmbeddingClient,
    reranker: OnnxReranker,
    result_count: usize,
}

impl LiteratureSearchService {
    pub fn new(
        metadata: MetadataStore,
        passages: PassageStore,
        embeddings: EmbeddingClient,
        reranker: OnnxReranker,
        result_count: usize,
    ) -> Result<Self, SearchError> {
        if result_count == 0 || result_count.checked_mul(4).is_none() {
            return Err(SearchError::InvalidInput(
                "SEARCH_RESULT_COUNT must be a positive value that can be multiplied by four"
                    .into(),
            ));
        }
        Ok(Self {
            metadata,
            passages,
            embeddings,
            reranker,
            result_count,
        })
    }

    pub async fn search(
        &self,
        query: &str,
        filters: &LiteratureFilters,
        include_metadata: bool,
    ) -> Result<LiteratureSearchResponse, SearchError> {
        validate_filters(filters)?;
        let eligible = self.metadata.eligible_pdf_hashes(filters).await?;
        if eligible.is_empty() {
            return Ok(LiteratureSearchResponse {
                results: Vec::new(),
                usage_note: usage_note().into(),
                metadata_by_pdf_hash: include_metadata.then(BTreeMap::new),
            });
        }
        let query_vector = self.embeddings.embed_query(query).await?;
        let candidates = self
            .passages
            .combined_candidates(query_vector, &eligible, self.result_count * 4)
            .await?;
        let texts = candidates
            .iter()
            .map(|candidate| candidate.text.clone())
            .collect::<Vec<_>>();
        let scores = if texts.is_empty() {
            Vec::new()
        } else {
            self.reranker.rerank(query, &texts).await?
        };
        if scores.len() != candidates.len() {
            return Err(SearchError::Rerank(format!(
                "returned {} scores for {} passages",
                scores.len(),
                candidates.len()
            )));
        }
        let results = rank_candidates(candidates, scores, self.result_count)
            .into_iter()
            .map(|candidate| LiteratureResult {
                text: candidate.text,
                pdf_hash: candidate.pdf_hash,
            })
            .collect::<Vec<_>>();
        let metadata_by_pdf_hash = if include_metadata {
            let hashes = results
                .iter()
                .map(|result| result.pdf_hash.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            Some(self.metadata.document_metadata(&hashes).await?)
        } else {
            None
        };
        Ok(LiteratureSearchResponse {
            results,
            usage_note: usage_note().into(),
            metadata_by_pdf_hash,
        })
    }

    pub async fn metadata(&self, hashes: &[String]) -> Result<MetadataResponse, SearchError> {
        let requested = hashes
            .iter()
            .map(|hash| hash.trim())
            .filter(|hash| !hash.is_empty())
            .map(str::to_owned)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if requested.is_empty() {
            return Err(SearchError::InvalidInput(
                "pdf_hashes must contain at least one non-empty value".into(),
            ));
        }
        let documents = self.metadata.document_metadata(&requested).await?;
        let not_found = requested
            .into_iter()
            .filter(|hash| !documents.contains_key(hash))
            .collect();
        Ok(MetadataResponse {
            documents,
            not_found,
        })
    }
}

fn usage_note() -> &'static str {
    "pdf_hash is an opaque identifier intended only for calls to other SCEPA MCP tools, such as get_document_metadata."
}

fn rank_candidates(
    candidates: Vec<crate::models::CombinedPassageCandidate>,
    scores: Vec<f32>,
    limit: usize,
) -> Vec<crate::models::CombinedPassageCandidate> {
    let mut ranked = candidates.into_iter().zip(scores).collect::<Vec<_>>();
    ranked.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .total_cmp(left_score)
            .then_with(|| left.point_id.cmp(&right.point_id))
    });
    ranked
        .into_iter()
        .take(limit)
        .map(|(candidate, _)| candidate)
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::models::CombinedPassageCandidate;

    use super::rank_candidates;

    #[test]
    fn reranked_candidates_are_limited_with_a_stable_tie_break() {
        let candidate = |id: &str| CombinedPassageCandidate {
            point_id: id.into(),
            pdf_hash: "hash".into(),
            text: id.into(),
        };
        let ranked = rank_candidates(
            vec![candidate("b"), candidate("c"), candidate("a")],
            vec![0.7, 0.9, 0.9],
            2,
        );
        assert_eq!(
            ranked
                .into_iter()
                .map(|candidate| candidate.point_id)
                .collect::<Vec<_>>(),
            ["a", "c"]
        );
    }
}
