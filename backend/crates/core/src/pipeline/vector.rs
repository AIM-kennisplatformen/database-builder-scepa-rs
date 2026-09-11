//! Source and combined passage embedding and vector synchronization.

use std::collections::BTreeSet;

use qdrant_client::qdrant::PointStruct;
use thiserror::Error;
use uuid::Uuid;

use crate::models::draft::{BoundingBox, Passage, TeiDocument};

use super::{
    embedding::{EmbeddingError, EmbeddingSource},
    qdrant::{
        CombinedPointPayloadData, PointPayloadData, QdrantStore, QdrantStoreError,
        SourcePointPayloadData, point_payload,
    },
};

const SOURCE_POINT_ID_NAMESPACE: &str = "scepa-document-passages-v1";
const COMBINED_POINT_ID_NAMESPACE: &str = "scepa-document-combined-passages-v1";
const COMBINED_TARGET_TOKENS: usize = 500;
const COMBINED_MAX_TOKENS: usize = 800;
const COMBINED_OVERLAP_TARGET_TOKENS: usize = 80;
const COMBINED_OVERLAP_MAX_TOKENS: usize = 100;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum VectorKind {
    Source,
    Combined,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PassageIdentity {
    kind: VectorKind,
    is_abstract: bool,
    id: String,
}

#[derive(Clone, Debug, PartialEq)]
struct EmbeddablePassage {
    identity: PassageIdentity,
    embedding_text: String,
    payload: PointPayloadData,
}

#[derive(Clone, Debug, PartialEq)]
struct SourcePassage {
    id: String,
    is_abstract: bool,
    text: String,
    coordinates: Vec<BoundingBox>,
    section: Option<String>,
    heading: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct CombinedPassage {
    id: String,
    is_abstract: bool,
    text: String,
    section: Option<String>,
    heading: Option<String>,
    source_indices: Vec<usize>,
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
        let points = prepare_document(pdf_hash, document)?;
        self.apply(pdf_hash, vec![], points).await
    }

    pub async fn update(
        &self,
        pdf_hash: &str,
        old_document: &TeiDocument,
        new_document: &TeiDocument,
    ) -> Result<(), VectorPipelineError> {
        let delete = prepare_document(pdf_hash, old_document)?
            .into_iter()
            .map(|point| point.identity)
            .collect();
        let upsert = prepare_document(pdf_hash, new_document)?;
        self.apply(pdf_hash, delete, upsert).await
    }

    async fn apply(
        &self,
        pdf_hash: &str,
        delete: Vec<PassageIdentity>,
        upsert: Vec<EmbeddablePassage>,
    ) -> Result<(), VectorPipelineError> {
        let texts: Vec<String> = upsert
            .iter()
            .map(|passage| passage.embedding_text.clone())
            .collect();
        // Embeddings are fully prepared and validated before Qdrant deletes
        // any stale points.
        let vectors = if texts.is_empty() {
            vec![]
        } else {
            self.embeddings.embed(&texts).await?
        };
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

        let delete_ids = delete
            .iter()
            .map(|identity| point_id(pdf_hash, identity).to_string())
            .collect();
        let points = upsert
            .into_iter()
            .zip(vectors)
            .map(|(passage, vector)| {
                PointStruct::new(
                    point_id(pdf_hash, &passage.identity).to_string(),
                    vector,
                    point_payload(&passage.payload),
                )
            })
            .collect();
        self.store.apply(delete_ids, points).await?;
        Ok(())
    }
}

fn prepare_document(
    pdf_hash: &str,
    document: &TeiDocument,
) -> Result<Vec<EmbeddablePassage>, VectorPipelineError> {
    let sources = collect_sources(document)?;
    let combined = build_combined(&sources);
    let mut combined_point_ids = vec![Vec::new(); sources.len()];
    for passage in &combined {
        let identity = PassageIdentity {
            kind: VectorKind::Combined,
            is_abstract: passage.is_abstract,
            id: passage.id.clone(),
        };
        let combined_point_id = point_id(pdf_hash, &identity).to_string();
        for &source_index in &passage.source_indices {
            combined_point_ids[source_index].push(combined_point_id.clone());
        }
    }

    let title = document.bibliography.title.as_deref();
    let mut points = Vec::with_capacity(sources.len() + combined.len());
    for (index, source) in sources.iter().enumerate() {
        let identity = PassageIdentity {
            kind: VectorKind::Source,
            is_abstract: source.is_abstract,
            id: source.id.clone(),
        };
        points.push(EmbeddablePassage {
            embedding_text: contextual_text(
                title,
                source.is_abstract,
                source.section.as_deref(),
                source.heading.as_deref(),
                &source.text,
            ),
            payload: PointPayloadData::Source(SourcePointPayloadData {
                pdf_hash: pdf_hash.to_owned(),
                is_abstract: source.is_abstract,
                is_combined: false,
                id: source.id.clone(),
                text: source.text.clone(),
                combined_point_ids: combined_point_ids[index].clone(),
                bounding_boxes: source.coordinates.clone(),
                section: source.section.clone(),
                heading: source.heading.clone(),
            }),
            identity,
        });
    }
    for passage in combined {
        let identity = PassageIdentity {
            kind: VectorKind::Combined,
            is_abstract: passage.is_abstract,
            id: passage.id.clone(),
        };
        points.push(EmbeddablePassage {
            embedding_text: contextual_text(
                title,
                passage.is_abstract,
                passage.section.as_deref(),
                passage.heading.as_deref(),
                &passage.text,
            ),
            payload: PointPayloadData::Combined(CombinedPointPayloadData {
                pdf_hash: pdf_hash.to_owned(),
                is_abstract: passage.is_abstract,
                is_combined: true,
                id: passage.id,
                text: passage.text,
                source_point_ids: passage
                    .source_indices
                    .iter()
                    .map(|&index| {
                        point_id(
                            pdf_hash,
                            &PassageIdentity {
                                kind: VectorKind::Source,
                                is_abstract: sources[index].is_abstract,
                                id: sources[index].id.clone(),
                            },
                        )
                        .to_string()
                    })
                    .collect(),
                section: passage.section,
                heading: passage.heading,
            }),
            identity,
        });
    }
    Ok(points)
}

fn collect_sources(document: &TeiDocument) -> Result<Vec<SourcePassage>, VectorPipelineError> {
    let mut identities = BTreeSet::new();
    let mut passages = Vec::new();
    for passage in &document.bibliography.abstract_text {
        insert_source(
            &mut passages,
            &mut identities,
            true,
            &passage.id,
            &passage.text,
            &passage.coordinates,
            passage.section.as_deref(),
            passage.heading_context.as_deref(),
        )?;
    }
    for passage in &document.body_text {
        let (id, text, coordinates, section, heading) = match passage {
            Passage::Text(passage) => (
                &passage.id,
                &passage.text,
                &passage.coordinates,
                passage.section.as_deref(),
                passage.heading_context.as_deref(),
            ),
            Passage::Formula(passage) => (
                &passage.id,
                &passage.text,
                &passage.coordinates,
                passage.section.as_deref(),
                passage.heading_context.as_deref(),
            ),
        };
        insert_source(
            &mut passages,
            &mut identities,
            false,
            id,
            text,
            coordinates,
            section,
            heading,
        )?;
    }
    Ok(passages)
}

#[allow(clippy::too_many_arguments)]
fn insert_source(
    passages: &mut Vec<SourcePassage>,
    identities: &mut BTreeSet<(bool, String)>,
    is_abstract: bool,
    id: &str,
    text: &str,
    coordinates: &[BoundingBox],
    section: Option<&str>,
    heading: Option<&str>,
) -> Result<(), VectorPipelineError> {
    if !identities.insert((is_abstract, id.to_owned())) {
        return Err(VectorPipelineError::DuplicateIdentity {
            kind: if is_abstract { "abstract" } else { "body" },
            id: id.to_owned(),
        });
    }
    if !text.trim().is_empty() {
        passages.push(SourcePassage {
            id: id.to_owned(),
            is_abstract,
            text: text.to_owned(),
            coordinates: coordinates.to_vec(),
            section: section.map(str::to_owned),
            heading: heading.map(str::to_owned),
        });
    }
    Ok(())
}

fn build_combined(sources: &[SourcePassage]) -> Vec<CombinedPassage> {
    let mut combined = Vec::new();
    let abstract_indices: Vec<_> = sources
        .iter()
        .enumerate()
        .filter_map(|(index, source)| source.is_abstract.then_some(index))
        .collect();
    append_context_groups(&mut combined, sources, &abstract_indices, true);

    let body_indices: Vec<_> = sources
        .iter()
        .enumerate()
        .filter_map(|(index, source)| (!source.is_abstract).then_some(index))
        .collect();
    append_context_groups(&mut combined, sources, &body_indices, false);
    combined
}

fn append_context_groups(
    combined: &mut Vec<CombinedPassage>,
    sources: &[SourcePassage],
    indices: &[usize],
    is_abstract: bool,
) {
    let mut group_start = 0;
    while group_start < indices.len() {
        let first = &sources[indices[group_start]];
        let mut group_end = group_start + 1;
        while group_end < indices.len() {
            let candidate = &sources[indices[group_end]];
            if candidate.section != first.section || candidate.heading != first.heading {
                break;
            }
            group_end += 1;
        }
        append_combined_group(
            combined,
            sources,
            &indices[group_start..group_end],
            is_abstract,
        );
        group_start = group_end;
    }
}

fn append_combined_group(
    combined: &mut Vec<CombinedPassage>,
    sources: &[SourcePassage],
    indices: &[usize],
    is_abstract: bool,
) {
    let mut cursor = 0;
    let mut overlap = Vec::new();
    while cursor < indices.len() {
        if !overlap.is_empty()
            && passage_tokens(sources, &overlap) + estimate_tokens(&sources[indices[cursor]].text)
                > COMBINED_MAX_TOKENS
        {
            overlap.clear();
        }
        let mut members = overlap.clone();
        let mut tokens = passage_tokens(sources, &members);
        while cursor < indices.len() {
            let next = indices[cursor];
            let next_tokens = estimate_tokens(&sources[next].text);
            if !members.is_empty() && tokens + next_tokens > COMBINED_MAX_TOKENS {
                break;
            }
            members.push(next);
            tokens += next_tokens;
            cursor += 1;
            if tokens >= COMBINED_TARGET_TOKENS {
                break;
            }
        }
        let ordinal = combined
            .iter()
            .filter(|passage| passage.is_abstract == is_abstract)
            .count()
            + 1;
        combined.push(make_combined(
            sources,
            members.clone(),
            is_abstract,
            ordinal,
        ));
        overlap = if cursor < indices.len() {
            overlap_suffix(sources, &members)
        } else {
            vec![]
        };
    }
}

fn make_combined(
    sources: &[SourcePassage],
    source_indices: Vec<usize>,
    is_abstract: bool,
    ordinal: usize,
) -> CombinedPassage {
    let first = &sources[source_indices[0]];
    let mut text = String::new();
    for &index in &source_indices {
        let source = &sources[index];
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&source.text);
    }
    CombinedPassage {
        id: format!(
            "combined_{}_{ordinal:08}",
            if is_abstract { "abstract" } else { "body" }
        ),
        is_abstract,
        text,
        section: first.section.clone(),
        heading: first.heading.clone(),
        source_indices,
    }
}

fn overlap_suffix(sources: &[SourcePassage], members: &[usize]) -> Vec<usize> {
    let mut tokens = 0;
    let mut best_start = members.len();
    let mut best_distance = COMBINED_OVERLAP_TARGET_TOKENS;
    for start in (0..members.len()).rev() {
        tokens += estimate_tokens(&sources[members[start]].text);
        if tokens > COMBINED_OVERLAP_MAX_TOKENS {
            break;
        }
        let distance = tokens.abs_diff(COMBINED_OVERLAP_TARGET_TOKENS);
        if distance <= best_distance {
            best_distance = distance;
            best_start = start;
        }
    }
    members[best_start..].to_vec()
}

fn passage_tokens(sources: &[SourcePassage], indices: &[usize]) -> usize {
    indices
        .iter()
        .map(|&index| estimate_tokens(&sources[index].text))
        .sum()
}

fn estimate_tokens(text: &str) -> usize {
    let mut tokens = 0;
    let mut ascii_run = 0;
    let flush_ascii = |tokens: &mut usize, run: &mut usize| {
        if *run > 0 {
            *tokens += (*run).div_ceil(4);
            *run = 0;
        }
    };
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            ascii_run += 1;
        } else {
            flush_ascii(&mut tokens, &mut ascii_run);
            if !character.is_whitespace() {
                tokens += 1;
            }
        }
    }
    flush_ascii(&mut tokens, &mut ascii_run);
    tokens
}

fn contextual_text(
    title: Option<&str>,
    is_abstract: bool,
    section: Option<&str>,
    heading: Option<&str>,
    text: &str,
) -> String {
    let mut context = Vec::new();
    if let Some(title) = title.filter(|value| !value.trim().is_empty()) {
        context.push(format!("Title: {}", title.trim()));
    }
    if is_abstract {
        context.push("Section: Abstract".into());
    } else if let Some(section) = section.filter(|value| !value.trim().is_empty()) {
        context.push(format!("Section: {}", section.trim()));
    }
    if let Some(heading) = heading.filter(|value| !value.trim().is_empty()) {
        context.push(format!("Heading: {}", heading.trim()));
    }
    if context.is_empty() {
        text.to_owned()
    } else {
        format!("{}\n\n{text}", context.join("\n"))
    }
}

fn point_id(pdf_hash: &str, identity: &PassageIdentity) -> Uuid {
    let namespace = match identity.kind {
        VectorKind::Source => SOURCE_POINT_ID_NAMESPACE,
        VectorKind::Combined => COMBINED_POINT_ID_NAMESPACE,
    };
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!(
            "{namespace}:{pdf_hash}:{}:{}",
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

    fn words(count: usize) -> String {
        std::iter::repeat_n("word", count)
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn bounding_box(page: u32, x: f64) -> BoundingBox {
        BoundingBox {
            page: Some(page),
            x,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        }
    }

    fn source_payload(point: &EmbeddablePassage) -> &SourcePointPayloadData {
        let PointPayloadData::Source(payload) = &point.payload else {
            panic!("expected source payload")
        };
        payload
    }

    fn combined_payload(point: &EmbeddablePassage) -> &CombinedPointPayloadData {
        let PointPayloadData::Combined(payload) = &point.payload else {
            panic!("expected combined payload")
        };
        payload
    }

    #[test]
    fn token_estimate_handles_ascii_unicode_and_punctuation() {
        assert_eq!(estimate_tokens("abcdefgh ij"), 3);
        assert_eq!(estimate_tokens("研究, ok"), 4);
        assert_eq!(estimate_tokens("   "), 0);
    }

    #[test]
    fn preparation_indexes_sources_combined_points_formulas_and_context() {
        let mut doc = document(
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
        doc.bibliography.title = Some("Paper title".into());
        let points = prepare_document("pdf", &doc).unwrap();
        assert_eq!(
            points
                .iter()
                .filter(|point| point.identity.kind == VectorKind::Source)
                .count(),
            3
        );
        assert_eq!(
            points
                .iter()
                .filter(|point| point.identity.kind == VectorKind::Combined)
                .count(),
            2
        );
        let abstract_source = points
            .iter()
            .find(|point| point.identity.id == "a1" && point.identity.kind == VectorKind::Source)
            .unwrap();
        assert_eq!(
            abstract_source.embedding_text,
            "Title: Paper title\nSection: Abstract\n\nabstract"
        );
        assert_eq!(source_payload(abstract_source).text, "abstract");
        assert_eq!(source_payload(abstract_source).combined_point_ids.len(), 1);
    }

    #[test]
    fn combined_windows_target_five_hundred_tokens_with_whole_passage_overlap() {
        let body = (1..=6)
            .map(|number| Passage::Text(text(&format!("p{number}"), &words(100))))
            .collect();
        let points = prepare_document("pdf", &document(vec![], body)).unwrap();
        let combined: Vec<_> = points
            .iter()
            .filter(|point| point.identity.kind == VectorKind::Combined)
            .collect();
        assert_eq!(combined.len(), 2);
        assert_eq!(combined_payload(combined[0]).source_point_ids.len(), 5);
        assert_eq!(combined_payload(combined[1]).source_point_ids.len(), 2);
        assert!(estimate_tokens(&combined_payload(combined[0]).text) <= COMBINED_MAX_TOKENS);
        let p5 = points
            .iter()
            .find(|point| point.identity.kind == VectorKind::Source && point.identity.id == "p5")
            .unwrap();
        assert_eq!(source_payload(p5).combined_point_ids.len(), 2);
    }

    #[test]
    fn maximum_is_soft_only_for_one_oversized_source_passage() {
        let body = vec![
            Passage::Text(text("p1", &words(450))),
            Passage::Text(text("p2", &words(400))),
            Passage::Text(text("manual", &words(900))),
        ];
        let points = prepare_document("pdf", &document(vec![], body)).unwrap();
        let combined: Vec<_> = points
            .iter()
            .filter(|point| point.identity.kind == VectorKind::Combined)
            .collect();
        assert_eq!(combined.len(), 3);
        assert!(estimate_tokens(&combined_payload(combined[0]).text) <= COMBINED_MAX_TOKENS);
        assert!(estimate_tokens(&combined_payload(combined[1]).text) <= COMBINED_MAX_TOKENS);
        assert_eq!(combined_payload(combined[2]).source_point_ids.len(), 1);
        assert_eq!(estimate_tokens(&combined_payload(combined[2]).text), 900);
    }

    #[test]
    fn combined_points_do_not_cross_section_or_heading_boundaries() {
        let mut first = text("p1", "one");
        first.section = Some("Methods".into());
        first.heading_context = Some("Setup".into());
        let mut second = text("p2", "two");
        second.section = Some("Methods".into());
        second.heading_context = Some("Evaluation".into());
        let mut third = text("p3", "three");
        third.section = Some("Results".into());
        third.heading_context = Some("Evaluation".into());
        let points = prepare_document(
            "pdf",
            &document(
                vec![],
                vec![
                    Passage::Text(first),
                    Passage::Text(second),
                    Passage::Text(third),
                ],
            ),
        )
        .unwrap();
        let combined: Vec<_> = points
            .iter()
            .filter(|point| point.identity.kind == VectorKind::Combined)
            .collect();
        assert_eq!(combined.len(), 3);
    }

    #[test]
    fn source_and_combined_references_are_qdrant_point_ids() {
        let first_box = bounding_box(1, 1.0);
        let mut first = text("p1", "complete first passage");
        first.coordinates = vec![first_box];
        let second = text("p2", "complete second passage");
        let points = prepare_document(
            "pdf",
            &document(vec![], vec![Passage::Text(first), Passage::Text(second)]),
        )
        .unwrap();
        let combined = points
            .iter()
            .find(|point| point.identity.kind == VectorKind::Combined)
            .unwrap();
        let combined_id = point_id("pdf", &combined.identity).to_string();
        for source in points
            .iter()
            .filter(|point| point.identity.kind == VectorKind::Source)
        {
            assert_eq!(
                source_payload(source).combined_point_ids.as_slice(),
                std::slice::from_ref(&combined_id)
            );
            assert!(
                combined_payload(combined)
                    .source_point_ids
                    .contains(&point_id("pdf", &source.identity).to_string())
            );
        }
        assert_eq!(
            source_payload(
                points
                    .iter()
                    .find(|point| point.identity.id == "p1")
                    .unwrap()
            )
            .bounding_boxes,
            [first_box]
        );
    }

    #[test]
    fn complete_short_abstract_is_one_combined_point() {
        let points = prepare_document(
            "pdf",
            &document(
                vec![text("a1", &words(300)), text("a2", &words(300))],
                vec![],
            ),
        )
        .unwrap();
        let combined: Vec<_> = points
            .iter()
            .filter(|point| point.identity.kind == VectorKind::Combined)
            .collect();
        assert_eq!(combined.len(), 1);
        assert_eq!(combined_payload(combined[0]).source_point_ids.len(), 2);
    }

    #[test]
    fn blank_passages_are_skipped_and_duplicate_identities_are_rejected() {
        let blank = document(vec![text("a1", "  ")], vec![]);
        assert!(prepare_document("pdf", &blank).unwrap().is_empty());

        let duplicate = document(
            vec![],
            vec![
                Passage::Text(text("p1", "one")),
                Passage::Text(text("p1", "two")),
            ],
        );
        assert!(matches!(
            prepare_document("pdf", &duplicate),
            Err(VectorPipelineError::DuplicateIdentity { .. })
        ));
    }

    #[test]
    fn point_ids_are_stable_and_include_kind_and_pdf_hash() {
        let body = PassageIdentity {
            kind: VectorKind::Source,
            is_abstract: false,
            id: "p1".into(),
        };
        let abstract_passage = PassageIdentity {
            kind: VectorKind::Source,
            is_abstract: true,
            id: "p1".into(),
        };
        let combined = PassageIdentity {
            kind: VectorKind::Combined,
            is_abstract: false,
            id: "p1".into(),
        };
        assert_eq!(point_id("pdf-a", &body), point_id("pdf-a", &body));
        assert_ne!(
            point_id("pdf-a", &body),
            point_id("pdf-a", &abstract_passage)
        );
        assert_ne!(point_id("pdf-a", &body), point_id("pdf-b", &body));
        assert_ne!(point_id("pdf-a", &body), point_id("pdf-a", &combined));
    }
}
