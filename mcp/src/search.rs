use std::collections::BTreeSet;

use async_trait::async_trait;
use thiserror::Error;

use crate::{
    embedding::EmbeddingClient,
    models::{LiteratureFilters, LiteratureResult, LiteratureSearchResponse},
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
}

impl LiteratureSearchService {
    pub fn new(
        metadata: MetadataStore,
        passages: PassageStore,
        embeddings: EmbeddingClient,
        reranker: OnnxReranker,
    ) -> Self {
        Self {
            metadata,
            passages,
            embeddings,
            reranker,
        }
    }

    pub async fn search(
        &self,
        query: &str,
        filters: &LiteratureFilters,
        top_k: usize,
    ) -> Result<LiteratureSearchResponse, SearchError> {
        validate_top_k(top_k)?;
        validate_filters(filters)?;
        let eligible = self.metadata.eligible_pdf_hashes(filters).await?;
        if eligible.is_empty() {
            return Ok(LiteratureSearchResponse {
                results: Vec::new(),
                usage_note: usage_note().into(),
                metadata_by_pdf_hash: Default::default(),
            });
        }
        let query_vector = self.embeddings.embed_query(query).await?;
        let candidates = self
            .passages
            .combined_candidates(query_vector, &eligible, top_k * 4)
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
        let results = rank_candidates(candidates, scores, top_k)
            .into_iter()
            .map(|(candidate, score)| LiteratureResult {
                text: candidate.text,
                pdf_hash: candidate.pdf_hash,
                score,
            })
            .collect::<Vec<_>>();
        let hashes = results
            .iter()
            .map(|result| result.pdf_hash.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let metadata_by_pdf_hash = self.metadata.document_metadata(&hashes).await?;
        Ok(LiteratureSearchResponse {
            results,
            usage_note: usage_note().into(),
            metadata_by_pdf_hash,
        })
    }
}

pub fn validate_top_k(top_k: usize) -> Result<(), SearchError> {
    if !(1..=50).contains(&top_k) {
        return Err(SearchError::InvalidInput(
            "top_k must be between 1 and 50".into(),
        ));
    }
    Ok(())
}

fn usage_note() -> &'static str {
    "pdf_hash is an opaque key that associates each passage with its entry in metadata_by_pdf_hash; it is not a user-facing citation or document identifier."
}

fn rank_candidates(
    candidates: Vec<crate::models::CombinedPassageCandidate>,
    scores: Vec<f32>,
    limit: usize,
) -> Vec<(crate::models::CombinedPassageCandidate, f32)> {
    let mut ranked = candidates.into_iter().zip(scores).collect::<Vec<_>>();
    ranked.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .total_cmp(left_score)
            .then_with(|| left.point_id.cmp(&right.point_id))
    });
    ranked.into_iter().take(limit).collect()
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
                .map(|(candidate, score)| (candidate.point_id, score))
                .collect::<Vec<_>>(),
            [("a".into(), 0.9), ("c".into(), 0.9)]
        );
    }
}
