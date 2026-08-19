//! Passage change detection, embedding, and vector-store synchronization.

use std::collections::BTreeMap;

use qdrant_client::qdrant::PointStruct;
use thiserror::Error;
use uuid::Uuid;

use crate::models::draft::{Passage, TeiDocument};

use super::{
    embedding::{EmbeddingError, EmbeddingSource},
    qdrant::{QdrantStore, QdrantStoreError, passage_payload},
};

const POINT_ID_NAMESPACE: &str = "scepa-document-passages-v1";

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PassageIdentity {
    is_abstract: bool,
    id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EmbeddablePassage {
    identity: PassageIdentity,
    text: String,
}

#[derive(Debug, Default, Eq, PartialEq)]
struct VectorChangeSet {
    delete: Vec<PassageIdentity>,
    upsert: Vec<EmbeddablePassage>,
}

#[derive(Debug, Error)]
pub enum VectorPipelineError {
    #[error("document contains duplicate passage identity ({kind}, {id})")]
    DuplicateIdentity { kind: &'static str, id: String },
    #[error(transparent)]
    Embedding(#[from] EmbeddingError),
    #[error(transparent)]
    Qdrant(#[from] QdrantStoreError),
}

impl VectorPipelineError {
    pub fn is_terminal(&self) -> bool {
        match self {
            Self::DuplicateIdentity { .. } => true,
            Self::Embedding(error) => error.is_terminal(),
            Self::Qdrant(error) => error.is_terminal(),
        }
    }
}

#[derive(Clone)]
pub struct DocumentVectorPipeline {
    embeddings: EmbeddingSource,
    store: QdrantStore,
}

impl DocumentVectorPipeline {
    pub fn new(embeddings: EmbeddingSource, store: QdrantStore) -> Self {
        Self { embeddings, store }
    }

    pub async fn publish(
        &self,
        pdf_hash: &str,
        document: &TeiDocument,
    ) -> Result<(), VectorPipelineError> {
        let changes = diff_documents(None, document)?;
        self.apply(pdf_hash, changes).await
    }

    pub async fn update(
        &self,
        pdf_hash: &str,
        old_document: &TeiDocument,
        new_document: &TeiDocument,
    ) -> Result<(), VectorPipelineError> {
        let changes = diff_documents(Some(old_document), new_document)?;
        self.apply(pdf_hash, changes).await
    }

    async fn apply(
        &self,
        pdf_hash: &str,
        changes: VectorChangeSet,
    ) -> Result<(), VectorPipelineError> {
        let texts: Vec<String> = changes
            .upsert
            .iter()
            .map(|passage| passage.text.clone())
            .collect();
        // Embeddings are fully prepared and validated before Qdrant deletes
        // any stale points.
        let vectors = self.embeddings.embed(&texts).await?;
        let expected = self.store.vector_dimension();
        for vector in &vectors {
            if vector.len() as u64 != expected {
                return Err(QdrantStoreError::VectorDimensionMismatch {
                    expected,
                    actual: vector.len(),
                }
                .into());
            }
        }

        let delete_ids = changes
            .delete
            .iter()
            .map(|identity| point_id(pdf_hash, identity).to_string())
            .collect();
        let points = changes
            .upsert
            .into_iter()
            .zip(vectors)
            .map(|(passage, vector)| {
                PointStruct::new(
                    point_id(pdf_hash, &passage.identity).to_string(),
                    vector,
                    passage_payload(pdf_hash, passage.identity.is_abstract, &passage.identity.id),
                )
            })
            .collect();
        self.store.apply(delete_ids, points).await?;
        Ok(())
    }
}

fn diff_documents(
    old_document: Option<&TeiDocument>,
    new_document: &TeiDocument,
) -> Result<VectorChangeSet, VectorPipelineError> {
    let old = old_document
        .map(collect_passages)
        .transpose()?
        .unwrap_or_default();
    let new = collect_passages(new_document)?;
    let mut changes = VectorChangeSet::default();

    for (identity, old_text) in &old {
        match new.get(identity) {
            None => changes.delete.push(identity.clone()),
            Some(new_text) if new_text != old_text => {
                changes.delete.push(identity.clone());
                changes.upsert.push(EmbeddablePassage {
                    identity: identity.clone(),
                    text: new_text.clone(),
                });
            }
            Some(_) => {}
        }
    }
    for (identity, text) in new {
        if !old.contains_key(&identity) {
            changes.upsert.push(EmbeddablePassage { identity, text });
        }
    }
    Ok(changes)
}

fn collect_passages(
    document: &TeiDocument,
) -> Result<BTreeMap<PassageIdentity, String>, VectorPipelineError> {
    let mut passages = BTreeMap::new();
    for passage in &document.bibliography.abstract_text {
        insert_passage(&mut passages, true, &passage.id, &passage.text)?;
    }
    for passage in &document.body_text {
        let (id, text) = match passage {
            Passage::Text(passage) => (&passage.id, &passage.text),
            Passage::Formula(passage) => (&passage.id, &passage.text),
        };
        insert_passage(&mut passages, false, id, text)?;
    }
    passages.retain(|_, text| !text.trim().is_empty());
    Ok(passages)
}

fn insert_passage(
    passages: &mut BTreeMap<PassageIdentity, String>,
    is_abstract: bool,
    id: &str,
    text: &str,
) -> Result<(), VectorPipelineError> {
    let identity = PassageIdentity {
        is_abstract,
        id: id.to_owned(),
    };
    if passages.contains_key(&identity) {
        return Err(VectorPipelineError::DuplicateIdentity {
            kind: if is_abstract { "abstract" } else { "body" },
            id: id.to_owned(),
        });
    }
    passages.insert(identity, text.to_owned());
    Ok(())
}

fn point_id(pdf_hash: &str, identity: &PassageIdentity) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!(
            "{POINT_ID_NAMESPACE}:{pdf_hash}:{}:{}",
            identity.is_abstract, identity.id
        )
        .as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::draft::{Bibliography, FormulaPassage, PassageLevel, TextPassage};

    fn text(id: &str, value: &str) -> TextPassage {
        TextPassage {
            id: id.into(),
            text: value.into(),
            coordinates: vec![],
            references: vec![],
            heading_context: None,
            section: None,
        }
    }

    fn document(abstracts: Vec<TextPassage>, body: Vec<Passage>) -> TeiDocument {
        TeiDocument {
            level: PassageLevel::Paragraph,
            bibliography: Bibliography {
                abstract_text: abstracts,
                ..Bibliography::default()
            },
            body_text: body,
            figures_and_tables: vec![],
            references: vec![],
        }
    }

    #[test]
    fn new_document_collects_abstract_text_and_formula_chunks() {
        let doc = document(
            vec![text("a1", "abstract")],
            vec![
                Passage::Text(text("p1", "body")),
                Passage::Formula(FormulaPassage {
                    id: "f1".into(),
                    text: "x = 1".into(),
                    label: None,
                    coordinates: vec![],
                    heading_context: None,
                    section: None,
                }),
            ],
        );
        let changes = diff_documents(None, &doc).unwrap();
        assert_eq!(changes.upsert.len(), 3);
        assert!(changes.delete.is_empty());
    }

    #[test]
    fn update_only_replaces_changed_text_and_removes_missing_passages() {
        let old = document(
            vec![text("a1", "old abstract")],
            vec![
                Passage::Text(text("same", "same")),
                Passage::Text(text("removed", "gone")),
            ],
        );
        let new = document(
            vec![text("a1", "new abstract")],
            vec![
                Passage::Text(text("same", "same")),
                Passage::Text(text("added", "new")),
            ],
        );
        let changes = diff_documents(Some(&old), &new).unwrap();
        assert_eq!(changes.delete.len(), 2);
        assert_eq!(changes.upsert.len(), 2);
        assert!(changes.upsert.iter().any(|row| row.identity.id == "a1"));
        assert!(changes.upsert.iter().any(|row| row.identity.id == "added"));
    }

    #[test]
    fn reclassification_is_a_remove_and_add() {
        let old = document(vec![], vec![Passage::Text(text("p1", "value"))]);
        let new = document(vec![text("p1", "value")], vec![]);
        let changes = diff_documents(Some(&old), &new).unwrap();
        assert_eq!(changes.delete.len(), 1);
        assert_eq!(changes.upsert.len(), 1);
        assert!(!changes.delete[0].is_abstract);
        assert!(changes.upsert[0].identity.is_abstract);
    }

    #[test]
    fn blank_passages_are_skipped_and_duplicate_identities_are_rejected() {
        let blank = document(vec![text("a1", "  ")], vec![]);
        assert!(diff_documents(None, &blank).unwrap().upsert.is_empty());

        let duplicate = document(
            vec![],
            vec![
                Passage::Text(text("p1", "one")),
                Passage::Text(text("p1", "two")),
            ],
        );
        assert!(matches!(
            diff_documents(None, &duplicate),
            Err(VectorPipelineError::DuplicateIdentity { .. })
        ));
    }

    #[test]
    fn point_ids_are_stable_and_include_kind_and_pdf_hash() {
        let body = PassageIdentity {
            is_abstract: false,
            id: "p1".into(),
        };
        let abstract_passage = PassageIdentity {
            is_abstract: true,
            id: "p1".into(),
        };
        assert_eq!(point_id("pdf-a", &body), point_id("pdf-a", &body));
        assert_ne!(
            point_id("pdf-a", &body),
            point_id("pdf-a", &abstract_passage)
        );
        assert_ne!(point_id("pdf-a", &body), point_id("pdf-b", &body));
    }
}
