use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use thiserror::Error;

use crate::{
    embedding::EmbeddingClient,
    models::{
        CombinedPassageCandidate, DocumentMetadata, LiteratureFilters, LiteratureSearchResponse,
        LiteratureSource,
    },
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
                sources: Vec::new(),
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
        let ranked = rank_candidates(candidates, scores, top_k);
        let hashes = ranked
            .iter()
            .map(|(candidate, _)| candidate.pdf_hash.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let metadata_by_pdf_hash = self.metadata.document_metadata(&hashes).await?;
        Ok(LiteratureSearchResponse {
            sources: group_sources(ranked, &metadata_by_pdf_hash)?,
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

fn rank_candidates(
    candidates: Vec<CombinedPassageCandidate>,
    scores: Vec<f32>,
    limit: usize,
) -> Vec<(CombinedPassageCandidate, f32)> {
    let mut ranked = candidates.into_iter().zip(scores).collect::<Vec<_>>();
    ranked.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .total_cmp(left_score)
            .then_with(|| left.point_id.cmp(&right.point_id))
    });
    ranked.into_iter().take(limit).collect()
}

fn group_sources(
    ranked: Vec<(CombinedPassageCandidate, f32)>,
    metadata_by_pdf_hash: &BTreeMap<String, DocumentMetadata>,
) -> Result<Vec<LiteratureSource>, SearchError> {
    let mut source_indexes = BTreeMap::<String, usize>::new();
    let mut sources = Vec::<LiteratureSource>::new();

    for (candidate, _) in ranked {
        if let Some(index) = source_indexes.get(&candidate.pdf_hash).copied() {
            sources[index].passages.push(candidate.text);
            continue;
        }

        let metadata = metadata_by_pdf_hash
            .get(&candidate.pdf_hash)
            .ok_or_else(|| {
                SearchError::TypeDb(
                    "retrieved evidence is missing required bibliographic metadata".into(),
                )
            })?;
        if metadata.ieee_reference.trim().is_empty() {
            return Err(SearchError::TypeDb(
                "retrieved evidence has an empty IEEE reference".into(),
            ));
        }

        source_indexes.insert(candidate.pdf_hash, sources.len());
        sources.push(LiteratureSource {
            ieee_reference: metadata.ieee_reference.clone(),
            description: metadata.description.clone(),
            passages: vec![candidate.text],
        });
    }

    Ok(sources)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::models::{CombinedPassageCandidate, DocumentMetadata};

    use super::{group_sources, rank_candidates};

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

    fn metadata(reference: &str) -> DocumentMetadata {
        DocumentMetadata {
            document_id: "document".into(),
            document_type: "report".into(),
            title: "Title".into(),
            description: None,
            ieee_reference: reference.into(),
            doi: None,
            isbn: Vec::new(),
            persons: Vec::new(),
            organizations: Vec::new(),
            contributors: Vec::new(),
            affiliations: Vec::new(),
            publication_events: Vec::new(),
        }
    }

    #[test]
    fn passages_are_grouped_by_document_in_first_ranked_order() {
        let candidate = |id: &str, hash: &str, text: &str| CombinedPassageCandidate {
            point_id: id.into(),
            pdf_hash: hash.into(),
            text: text.into(),
        };
        let ranked = vec![
            (
                candidate("1", "second", "Second document first passage"),
                0.9,
            ),
            (candidate("2", "first", "First document passage"), 0.8),
            (
                candidate("3", "second", "Second document next passage"),
                0.7,
            ),
        ];
        let metadata_by_pdf_hash = BTreeMap::from([
            ("first".into(), metadata("First reference.")),
            ("second".into(), metadata("Second reference.")),
        ]);

        let sources = group_sources(ranked, &metadata_by_pdf_hash).unwrap();

        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].ieee_reference, "Second reference.");
        assert_eq!(
            sources[0].passages,
            [
                "Second document first passage",
                "Second document next passage"
            ]
        );
        assert_eq!(sources[1].ieee_reference, "First reference.");
    }

    #[test]
    fn grouping_rejects_evidence_without_a_usable_reference() {
        let candidate = CombinedPassageCandidate {
            point_id: "1".into(),
            pdf_hash: "missing".into(),
            text: "Evidence".into(),
        };

        let missing = group_sources(vec![(candidate.clone(), 0.9)], &BTreeMap::new());
        assert!(missing.is_err());

        let empty_metadata = BTreeMap::from([("missing".into(), metadata("  "))]);
        let empty = group_sources(vec![(candidate, 0.9)], &empty_metadata);
        assert!(empty.is_err());
    }
}
