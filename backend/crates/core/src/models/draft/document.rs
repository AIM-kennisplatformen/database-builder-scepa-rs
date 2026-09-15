use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::identity::{is_uuid, stable_id};
use crate::models::draft::{
    bibliography::{Bibliography, Contributor, Identifier},
    figure::FigureOrTable,
    passage::Passage,
};

/// Whether prose was segmented into paragraphs or sentences by the TEI producer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PassageLevel {
    Paragraph,
    Sentence,
}

/// A complete application-facing representation of a TEI document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema)]
pub struct TeiDocument {
    #[serde(default)]
    pub id: String,
    pub level: PassageLevel,
    pub bibliography: Bibliography,
    pub body_text: Vec<Passage>,
    pub figures_and_tables: Vec<FigureOrTable>,
}

/// An extraction draft together with sparse, operator-authored overrides.
///
/// The serialized field names intentionally describe the persisted artifact,
/// while the Rust names describe how the values are used in the application.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DraftDocument {
    #[serde(rename = "grobid_extraction_data")]
    pub extracted_data: TeiDocument,
    pub manual_data: ManualDocument,
}

impl DraftDocument {
    pub fn new(extracted_data: TeiDocument) -> Self {
        Self {
            extracted_data,
            manual_data: ManualDocument::default(),
        }
    }

    /// Produces the TEI-shaped input used for canonicalisation.
    ///
    /// Extraction evidence is retained in `self`; only this derived value is
    /// overlaid. A populated manual field always wins, including an empty list.
    pub fn effective_document(&self) -> TeiDocument {
        let mut effective = self.extracted_data.clone();
        let manual = &self.manual_data.bibliography;

        if let Some(value) = &manual.title {
            effective.bibliography.title = Some(value.clone());
        }
        if let Some(value) = &manual.authors {
            effective.bibliography.authors = value.clone();
        }
        if let Some(value) = &manual.identifiers {
            effective.bibliography.identifiers = value.clone();
        }
        if let Some(value) = &manual.publication_date {
            effective.bibliography.publication_date = Some(value.clone());
        }
        if let Some(value) = manual.publication_year {
            effective.bibliography.publication_year = Some(value);
        }
        if let Some(value) = &manual.publisher {
            effective.bibliography.publisher = Some(value.clone());
        }
        if let Some(value) = &manual.journal {
            effective.bibliography.journal = Some(value.clone());
        }
        if let Some(value) = &manual.journal_abbreviation {
            effective.bibliography.journal_abbreviation = Some(value.clone());
        }
        if let Some(value) = &manual.publication_event_id {
            effective.bibliography.publication_event_id = Some(value.clone());
        }
        if let Some(value) = &manual.abstract_text {
            effective.bibliography.abstract_text = value.clone();
        }
        if let Some(value) = &self.manual_data.body_text {
            effective.body_text = value.clone();
        }

        effective
    }

    pub fn assign_extracted_ids(&mut self, pdf_hash: &str) {
        self.extracted_data.assign_extracted_ids(pdf_hash);
    }

    pub fn assign_manual_ids(&mut self, pdf_hash: &str, idempotency_key: &str) {
        self.manual_data
            .assign_missing_ids(pdf_hash, idempotency_key);
    }

    pub fn validate_ids(&self) -> Result<(), String> {
        self.extracted_data.validate_ids()?;
        let effective = self.effective_document();
        effective.validate_ids()
    }
}

impl TeiDocument {
    pub fn assign_extracted_ids(&mut self, pdf_hash: &str) {
        let scope = format!("pdf:{pdf_hash}");
        self.id = stable_id(&scope, "document", "root");

        assign_bibliography_ids(&mut self.bibliography, &scope, "bibliography");

        for passage in &mut self.body_text {
            let (kind, id) = match passage {
                Passage::Text(value) => ("text-passage", &mut value.id),
                Passage::Formula(value) => ("formula-passage", &mut value.id),
            };
            if !is_uuid(id) {
                let locator = id.clone();
                *id = stable_id(&scope, kind, &locator);
            }
        }
        for passage in &mut self.bibliography.abstract_text {
            if !is_uuid(&passage.id) {
                let locator = passage.id.clone();
                passage.id = stable_id(&scope, "abstract-passage", &locator);
            }
        }
        for media in &mut self.figures_and_tables {
            let (kind, id) = match media {
                crate::models::draft::FigureOrTable::Figure(value) => ("figure", &mut value.id),
                crate::models::draft::FigureOrTable::Table(value) => ("table", &mut value.id),
            };
            if !is_uuid(id) {
                let locator = id.clone();
                *id = stable_id(&scope, kind, &locator);
            }
        }
    }

    pub fn validate_ids(&self) -> Result<(), String> {
        let mut seen = HashSet::new();
        validate_id(&self.id, "document", &mut seen)?;
        validate_bibliography_ids(&self.bibliography, &mut seen)?;
        for passage in &self.body_text {
            let id = match passage {
                Passage::Text(value) => &value.id,
                Passage::Formula(value) => &value.id,
            };
            validate_id(id, "passage", &mut seen)?;
        }
        for passage in &self.bibliography.abstract_text {
            validate_id(&passage.id, "abstract passage", &mut seen)?;
        }
        for media in &self.figures_and_tables {
            let id = match media {
                crate::models::draft::FigureOrTable::Figure(value) => &value.id,
                crate::models::draft::FigureOrTable::Table(value) => &value.id,
            };
            validate_id(id, "media", &mut seen)?;
        }
        Ok(())
    }
}

impl ManualDocument {
    pub fn requires_identity_assignment(&self) -> bool {
        self.bibliography
            .authors
            .as_ref()
            .is_some_and(|authors| authors.iter().any(contributor_requires_ids))
            || self
                .bibliography
                .publisher
                .as_ref()
                .is_some_and(|value| value.id.is_empty())
            || self
                .bibliography
                .journal
                .as_ref()
                .is_some_and(|value| value.id.is_empty())
            || ((self.bibliography.publication_date.is_some()
                || self.bibliography.publication_year.is_some())
                && self
                    .bibliography
                    .publication_event_id
                    .as_deref()
                    .unwrap_or_default()
                    .is_empty())
            || self.body_text.as_ref().is_some_and(|passages| {
                passages.iter().any(|passage| match passage {
                    Passage::Text(value) => value.id.is_empty(),
                    Passage::Formula(value) => value.id.is_empty(),
                })
            })
            || self
                .bibliography
                .abstract_text
                .as_ref()
                .is_some_and(|passages| passages.iter().any(|passage| passage.id.is_empty()))
    }

    pub fn assign_missing_ids(&mut self, pdf_hash: &str, idempotency_key: &str) {
        let scope = format!("pdf:{pdf_hash}:request:{idempotency_key}");
        if let Some(authors) = &mut self.bibliography.authors {
            let mut organization_ids = HashMap::<String, String>::new();
            for (index, contributor) in authors.iter_mut().enumerate() {
                let organization_was_missing = contributor
                    .affiliation
                    .as_ref()
                    .is_some_and(|value| value.organization.id.is_empty());
                assign_contributor_ids(contributor, &scope, &format!("author:{}", index + 1));
                if let Some(affiliation) = &mut contributor.affiliation {
                    let key = normalize_identity_value(&affiliation.organization.name);
                    if organization_was_missing {
                        let id = organization_ids.entry(key).or_insert_with(|| {
                            stable_id(
                                &scope,
                                "organization",
                                &format!("affiliation:{}", index + 1),
                            )
                        });
                        affiliation.organization.id = id.clone();
                    } else {
                        organization_ids
                            .entry(key)
                            .or_insert_with(|| affiliation.organization.id.clone());
                    }
                }
            }
        }
        if let Some(publisher) = &mut self.bibliography.publisher
            && publisher.id.is_empty()
        {
            publisher.id = stable_id(&scope, "organization", "publisher");
        }
        if let Some(journal) = &mut self.bibliography.journal
            && journal.id.is_empty()
        {
            journal.id = stable_id(&scope, "publication-venue", "journal");
        }
        if (self.bibliography.publication_date.is_some()
            || self.bibliography.publication_year.is_some())
            && self
                .bibliography
                .publication_event_id
                .as_deref()
                .unwrap_or_default()
                .is_empty()
        {
            self.bibliography.publication_event_id =
                Some(stable_id(&scope, "publication-event", "publication"));
        }
        if let Some(passages) = &mut self.body_text {
            for (index, passage) in passages.iter_mut().enumerate() {
                let (kind, id) = match passage {
                    Passage::Text(value) => ("text-passage", &mut value.id),
                    Passage::Formula(value) => ("formula-passage", &mut value.id),
                };
                if id.is_empty() {
                    *id = stable_id(&scope, kind, &format!("body:{}", index + 1));
                }
            }
        }
        if let Some(passages) = &mut self.bibliography.abstract_text {
            for (index, passage) in passages.iter_mut().enumerate() {
                if passage.id.is_empty() {
                    passage.id = stable_id(
                        &scope,
                        "abstract-passage",
                        &format!("abstract:{}", index + 1),
                    );
                }
            }
        }
    }
}

fn assign_bibliography_ids(bibliography: &mut Bibliography, scope: &str, locator: &str) {
    let mut occurrences = HashMap::<String, usize>::new();
    let mut organization_ids = HashMap::<String, String>::new();
    for contributor in &mut bibliography.authors {
        let fingerprint = contributor_fingerprint(contributor);
        let occurrence = occurrences.entry(fingerprint.clone()).or_default();
        *occurrence += 1;
        assign_contributor_ids(
            contributor,
            scope,
            &format!("{locator}:author:{fingerprint}:{}", *occurrence),
        );
        if let Some(affiliation) = &mut contributor.affiliation {
            let normalized_name = normalize_identity_value(&affiliation.organization.name);
            if affiliation.organization.id.is_empty() {
                let organization_id = organization_ids
                    .entry(normalized_name.clone())
                    .or_insert_with(|| {
                        stable_id(
                            scope,
                            "organization",
                            &format!("{locator}:organization:{normalized_name}"),
                        )
                    });
                affiliation.organization.id = organization_id.clone();
            } else {
                organization_ids
                    .entry(normalized_name)
                    .or_insert_with(|| affiliation.organization.id.clone());
            }
        }
    }
    if let Some(publisher) = &mut bibliography.publisher
        && publisher.id.is_empty()
    {
        publisher.id = stable_id(scope, "organization", &format!("{locator}:publisher"));
    }
    if let Some(journal) = &mut bibliography.journal
        && journal.id.is_empty()
    {
        journal.id = stable_id(scope, "publication-venue", &format!("{locator}:journal"));
    }
    if (bibliography.publication_date.is_some() || bibliography.publication_year.is_some())
        && bibliography
            .publication_event_id
            .as_deref()
            .unwrap_or_default()
            .is_empty()
    {
        bibliography.publication_event_id = Some(stable_id(
            scope,
            "publication-event",
            &format!("{locator}:publication"),
        ));
    }
}

fn contributor_fingerprint(contributor: &Contributor) -> String {
    format!(
        "{}|{}|{}|{}|{:?}",
        normalize_identity_value(&contributor.name),
        normalize_identity_value(contributor.forename.as_deref().unwrap_or_default()),
        normalize_identity_value(contributor.surname.as_deref().unwrap_or_default()),
        contributor
            .affiliation
            .as_ref()
            .map(|value| normalize_identity_value(&value.organization.name))
            .unwrap_or_default(),
        contributor.role,
    )
}

fn normalize_identity_value(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn assign_contributor_ids(contributor: &mut Contributor, scope: &str, locator: &str) {
    if contributor.id.is_empty() {
        contributor.id = stable_id(scope, "person", locator);
    }
    if contributor.contribution_id.is_empty() {
        contributor.contribution_id = stable_id(scope, "contribution", locator);
    }
    if let Some(affiliation) = &mut contributor.affiliation {
        if affiliation.id.is_empty() {
            affiliation.id = stable_id(scope, "affiliation", locator);
        }
        if affiliation.organization.id.is_empty() {
            affiliation.organization.id =
                stable_id(scope, "organization", &format!("{locator}:organization"));
        }
    }
}

fn contributor_requires_ids(contributor: &Contributor) -> bool {
    contributor.id.is_empty()
        || contributor.contribution_id.is_empty()
        || contributor
            .affiliation
            .as_ref()
            .is_some_and(|value| value.id.is_empty() || value.organization.id.is_empty())
}

fn validate_bibliography_ids(
    bibliography: &Bibliography,
    seen: &mut HashSet<String>,
) -> Result<(), String> {
    for contributor in &bibliography.authors {
        validate_contributor_ids(contributor, seen)?;
    }
    if let Some(value) = &bibliography.publisher {
        validate_id(&value.id, "publisher", seen)?;
    }
    if let Some(value) = &bibliography.journal {
        validate_id(&value.id, "publication venue", seen)?;
    }
    if let Some(value) = &bibliography.publication_event_id {
        validate_id(value, "publication event", seen)?;
    }
    Ok(())
}

fn validate_contributor_ids(
    contributor: &Contributor,
    seen: &mut HashSet<String>,
) -> Result<(), String> {
    validate_id(&contributor.id, "contributor", seen)?;
    validate_id(&contributor.contribution_id, "contribution", seen)?;
    if let Some(value) = &contributor.affiliation {
        validate_id(&value.id, "affiliation", seen)?;
        if !is_uuid(&value.organization.id) {
            return Err("affiliation organization ID is not a UUID".into());
        }
    }
    Ok(())
}

fn validate_id(value: &str, label: &str, seen: &mut HashSet<String>) -> Result<(), String> {
    if !is_uuid(value) {
        return Err(format!("{label} ID is not a UUID"));
    }
    if !seen.insert(value.to_owned()) {
        return Err(format!("duplicate ID {value}"));
    }
    Ok(())
}

/// Human-authored values that may override extraction for canonicalisation.
///
/// Besides metadata, operators can correct the abstract/body classification and
/// the text or source coordinates of passages. The extracted layer remains
/// immutable; populated passage fields replace it only in the effective view.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default)]
pub struct ManualDocument {
    pub bibliography: ManualBibliography,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_text: Option<Vec<Passage>>,
}

/// Sparse bibliography patch. `None` means “use the extracted value”; for
/// collections, `Some(vec![])` intentionally replaces extraction with no rows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default)]
pub struct ManualBibliography {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<Contributor>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifiers: Option<Vec<Identifier>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication_year: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<crate::models::draft::bibliography::DraftOrganization>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journal: Option<crate::models::draft::bibliography::DraftPublicationVenue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journal_abbreviation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstract_text: Option<Vec<crate::models::draft::passage::TextPassage>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contributor(name: &str) -> Contributor {
        Contributor {
            id: String::new(),
            contribution_id: String::new(),
            name: name.into(),
            forename: Some(name.into()),
            surname: Some("Example".into()),
            affiliation: Some("Example University".into()),
            role: crate::models::draft::ContributorRole::Author,
        }
    }

    fn extracted() -> TeiDocument {
        TeiDocument {
            id: String::new(),
            level: PassageLevel::Paragraph,
            bibliography: Bibliography {
                title: Some("Extracted title".into()),
                ..Bibliography::default()
            },
            body_text: vec![],
            figures_and_tables: vec![],
        }
    }

    #[test]
    fn artifact_uses_storage_contract_field_names() {
        let value = serde_json::to_value(DraftDocument::new(extracted())).unwrap();
        assert!(value.get("grobid_extraction_data").is_some());
        assert_eq!(
            value["manual_data"],
            serde_json::json!({ "bibliography": {} })
        );
        assert!(value.get("extracted_data").is_none());
    }

    #[test]
    fn legacy_citation_fields_are_ignored() {
        let mut value = serde_json::to_value(extracted()).unwrap();
        value["references"] = serde_json::json!([{ "title": "Prior work" }]);
        value["body_text"] = serde_json::json!([{
            "type": "text",
            "id": "p1",
            "text": "Body text",
            "coordinates": [],
            "references": [{
                "target": "#b1",
                "text": "[1]",
                "byte_start": 5,
                "byte_end": 8
            }],
            "heading_context": null,
            "section": null
        }]);

        let document: TeiDocument = serde_json::from_value(value).unwrap();
        let serialized = serde_json::to_value(document).unwrap();
        assert!(serialized.get("references").is_none());
        assert!(serialized["body_text"][0].get("references").is_none());
    }

    #[test]
    fn manual_values_overlay_without_mutating_extraction() {
        let mut draft = DraftDocument::new(extracted());
        draft.manual_data.bibliography.title = Some("Reviewed title".into());
        draft.manual_data.bibliography.authors = Some(vec![]);
        draft.manual_data.body_text = Some(vec![]);
        draft.manual_data.bibliography.abstract_text = Some(vec![]);

        let effective = draft.effective_document();
        assert_eq!(
            effective.bibliography.title.as_deref(),
            Some("Reviewed title")
        );
        assert!(effective.bibliography.authors.is_empty());
        assert!(effective.body_text.is_empty());
        assert!(effective.bibliography.abstract_text.is_empty());
        assert_eq!(
            draft.extracted_data.bibliography.title.as_deref(),
            Some("Extracted title")
        );
    }

    #[test]
    fn extracted_ids_are_deterministic_scoped_and_valid() {
        let hash = "a".repeat(64);
        let mut first = extracted();
        first.bibliography.authors = vec![contributor("Ada"), contributor("Grace")];
        first.bibliography.publication_date = Some("2024-05-06".into());
        first.bibliography.publisher = Some("Example Press".into());
        first.bibliography.journal = Some("Example Journal".into());
        let mut second = first.clone();

        first.assign_extracted_ids(&hash);
        second.assign_extracted_ids(&hash);

        assert_eq!(first, second);
        let assigned = first.clone();
        first.assign_extracted_ids(&hash);
        assert_eq!(first, assigned);
        first.validate_ids().unwrap();
        assert_ne!(first.id, first.bibliography.authors[0].id);

        let mut other_pdf = extracted();
        other_pdf.bibliography.authors = vec![contributor("Ada"), contributor("Grace")];
        other_pdf.bibliography.publication_date = Some("2024-05-06".into());
        other_pdf.bibliography.publisher = Some("Example Press".into());
        other_pdf.bibliography.journal = Some("Example Journal".into());
        other_pdf.assign_extracted_ids(&"b".repeat(64));
        assert_ne!(first.id, other_pdf.id);
        assert_ne!(
            first.bibliography.authors[0].id,
            other_pdf.bibliography.authors[0].id
        );
    }

    #[test]
    fn contributor_ids_survive_edits_and_reordering() {
        let mut document = extracted();
        document.bibliography.authors = vec![contributor("Ada"), contributor("Grace")];
        document.assign_extracted_ids(&"a".repeat(64));
        let grace_id = document.bibliography.authors[1].id.clone();
        let grace_contribution_id = document.bibliography.authors[1].contribution_id.clone();

        document.bibliography.authors.reverse();
        document.bibliography.authors[0].name = "Grace Hopper".into();

        assert_eq!(document.bibliography.authors[0].id, grace_id);
        assert_eq!(
            document.bibliography.authors[0].contribution_id,
            grace_contribution_id
        );
    }

    #[test]
    fn manual_creation_is_idempotent_but_new_keys_create_new_identity() {
        let mut request = ManualDocument::default();
        request.bibliography.authors = Some(vec![contributor("Ada")]);
        assert!(request.requires_identity_assignment());

        let mut retry = request.clone();
        request.assign_missing_ids(&"a".repeat(64), "request-1");
        retry.assign_missing_ids(&"a".repeat(64), "request-1");
        assert_eq!(request, retry);
        assert!(!request.requires_identity_assignment());

        let mut recreated = ManualDocument::default();
        recreated.bibliography.authors = Some(vec![contributor("Ada")]);
        recreated.assign_missing_ids(&"a".repeat(64), "request-2");
        assert_ne!(
            request.bibliography.authors.as_ref().unwrap()[0].id,
            recreated.bibliography.authors.as_ref().unwrap()[0].id
        );
    }

    #[test]
    fn invalid_and_duplicate_ids_are_rejected() {
        let mut document = extracted();
        document.bibliography.authors = vec![contributor("Ada")];
        document.bibliography.authors[0].id.clear();
        document.assign_extracted_ids(&"a".repeat(64));

        document.bibliography.authors[0].id = "not-a-uuid".into();
        assert_eq!(
            document.validate_ids().unwrap_err(),
            "contributor ID is not a UUID"
        );

        document.bibliography.authors[0].id.clear();
        document.assign_extracted_ids(&"a".repeat(64));
        document.bibliography.authors[0].contribution_id = document.id.clone();
        assert!(
            document
                .validate_ids()
                .unwrap_err()
                .starts_with("duplicate ID")
        );
    }
}
