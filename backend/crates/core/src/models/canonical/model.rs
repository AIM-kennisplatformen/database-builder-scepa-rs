//! Root canonical model and conversion from the extraction draft.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, NaiveDateTime};
use sha2::{Digest, Sha256};

use super::entities::document::{Book, Document, Report, ResearchPaper};
use super::entities::organization::{
    EducationalInstitution, GovernmentInstitution, Institution, NonprofitInstitution, Organization,
    Publisher,
};
use super::entities::person::Person;
use super::entities::publication_venue::{Conference, Journal};
use crate::models::draft::{ContributorRole, Identifier, IdentifierKind, TeiDocument};

/// The complete canonical value accepted by persistence services.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalModel {
    pub document: CanonicalDocument,
    pub contributors: Vec<CanonicalContributor>,
    pub organizations: Vec<CanonicalOrganization>,
    pub publication_venues: Vec<CanonicalPublicationVenue>,
    pub affiliations: Vec<CanonicalAffiliation>,
    pub publication_events: Vec<CanonicalPublicationEvent>,
}

/// A canonical person and their contribution to the model's document.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalContributor {
    pub person: Person,
    pub contribution: CanonicalContribution,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalContribution {
    Authorship,
    Contribution,
    PeerReview,
}

/// An organization entity supported by the TypeDB schema.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CanonicalOrganization {
    Organization(Organization),
    Institution(Institution),
    GovernmentInstitution(GovernmentInstitution),
    EducationalInstitution(EducationalInstitution),
    NonprofitInstitution(NonprofitInstitution),
    Publisher(Publisher),
}

impl CanonicalOrganization {
    pub fn organization_id(&self) -> &str {
        match self {
            Self::Organization(value) => &value.organization_id,
            Self::Institution(value) => &value.organization_id,
            Self::GovernmentInstitution(value) => &value.organization_id,
            Self::EducationalInstitution(value) => &value.organization_id,
            Self::NonprofitInstitution(value) => &value.organization_id,
            Self::Publisher(value) => &value.organization_id,
        }
    }

    pub fn organization_name(&self) -> &str {
        match self {
            Self::Organization(value) => &value.organization_name,
            Self::Institution(value) => &value.organization_name,
            Self::GovernmentInstitution(value) => &value.organization_name,
            Self::EducationalInstitution(value) => &value.organization_name,
            Self::NonprofitInstitution(value) => &value.organization_name,
            Self::Publisher(value) => &value.organization_name,
        }
    }

    pub fn ror_id(&self) -> Option<&str> {
        match self {
            Self::Organization(value) => value.ror_id.as_deref(),
            Self::Institution(value) => value.ror_id.as_deref(),
            Self::GovernmentInstitution(value) => value.ror_id.as_deref(),
            Self::EducationalInstitution(value) => value.ror_id.as_deref(),
            Self::NonprofitInstitution(value) => value.ror_id.as_deref(),
            Self::Publisher(value) => value.ror_id.as_deref(),
        }
    }

    pub fn entity_type(&self) -> &'static str {
        match self {
            Self::Organization(_) => "organization",
            Self::Institution(_) => "institution",
            Self::GovernmentInstitution(_) => "government_institution",
            Self::EducationalInstitution(_) => "educational_institution",
            Self::NonprofitInstitution(_) => "nonprofit_institution",
            Self::Publisher(_) => "publisher",
        }
    }
}

/// A publication venue entity supported by the TypeDB schema.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CanonicalPublicationVenue {
    Journal(Journal),
    Conference(Conference),
}

impl CanonicalPublicationVenue {
    pub fn venue_id(&self) -> &str {
        match self {
            Self::Journal(value) => &value.venue_id,
            Self::Conference(value) => &value.venue_id,
        }
    }

    pub fn venue_name(&self) -> &str {
        match self {
            Self::Journal(value) => &value.venue_name,
            Self::Conference(value) => &value.venue_name,
        }
    }

    pub fn issn(&self) -> Option<&str> {
        match self {
            Self::Journal(value) => value.issn.as_deref(),
            Self::Conference(value) => value.issn.as_deref(),
        }
    }

    pub fn entity_type(&self) -> &'static str {
        match self {
            Self::Journal(_) => "journal",
            Self::Conference(_) => "conference",
        }
    }
}

/// ID-linked representation of the schema's affiliation relation.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalAffiliation {
    pub person_id: String,
    pub organization_id: String,
    pub evidence_document_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalPublicationEventKind {
    Submission,
    Acceptance,
    Publication,
}

/// ID-linked representation of a publication-event relation.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalPublicationEvent {
    pub kind: CanonicalPublicationEventKind,
    pub work_document_id: String,
    pub publisher_id: Option<String>,
    pub venue_id: Option<String>,
    pub publication_date: NaiveDateTime,
    pub publication_notes: Vec<String>,
    pub version_number: Option<String>,
}

/// A document in the canonical domain model.
///
/// The variant determines the TypeDB entity type used when the model is
/// persisted. More specialised variants retain their identifying attribute.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CanonicalDocument {
    Document(Document),
    ResearchPaper(ResearchPaper),
    Book(Book),
    Report(Report),
}

impl CanonicalDocument {
    pub fn document_id(&self) -> &str {
        match self {
            Self::Document(document) => &document.document_id,
            Self::ResearchPaper(document) => &document.document_id,
            Self::Book(document) => &document.document_id,
            Self::Report(document) => &document.document_id,
        }
    }

    pub fn pdf_hash(&self) -> Option<&str> {
        match self {
            Self::Document(document) => document.pdf_hash.as_deref(),
            Self::ResearchPaper(document) => document.pdf_hash.as_deref(),
            Self::Book(document) => document.pdf_hash.as_deref(),
            Self::Report(document) => document.pdf_hash.as_deref(),
        }
    }

    pub fn set_pdf_hash(&mut self, pdf_hash: String) {
        match self {
            Self::Document(document) => document.pdf_hash = Some(pdf_hash),
            Self::ResearchPaper(document) => document.pdf_hash = Some(pdf_hash),
            Self::Book(document) => document.pdf_hash = Some(pdf_hash),
            Self::Report(document) => document.pdf_hash = Some(pdf_hash),
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::Document(document) => &document.title,
            Self::ResearchPaper(document) => &document.title,
            Self::Book(document) => &document.title,
            Self::Report(document) => &document.title,
        }
    }

    pub fn entity_type(&self) -> &'static str {
        match self {
            Self::Document(_) => "document",
            Self::ResearchPaper(_) => "research_paper",
            Self::Book(_) => "book",
            Self::Report(_) => "report",
        }
    }

    fn try_from_draft(
        draft: &TeiDocument,
        fallback_document_id: Option<String>,
    ) -> eros::Result<Self> {
        let Some(title) = draft
            .bibliography
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
        else {
            eros::bail!("canonical document requires a title")
        };
        let title = title.to_owned();

        let doi = identifier(&draft.bibliography.identifiers, |kind| {
            matches!(kind, IdentifierKind::Doi)
        });
        let isbn = identifier(&draft.bibliography.identifiers, |kind| {
            matches!(kind, IdentifierKind::Isbn)
        });
        if let Some(doi) = doi {
            return Ok(Self::ResearchPaper(ResearchPaper {
                document_id: doi.to_owned(),
                pdf_hash: None,
                title,
                doi: Some(doi.to_owned()),
            }));
        }

        if let Some(isbn) = isbn {
            return Ok(Self::Book(Book {
                document_id: isbn.to_owned(),
                pdf_hash: None,
                title,
                isbn: Some(isbn.to_owned()),
            }));
        }

        let Some(document_id) = draft
            .bibliography
            .identifiers
            .iter()
            .filter(|identifier| {
                !matches!(identifier.kind, IdentifierKind::Issn | IdentifierKind::Md5)
            })
            .map(|identifier| identifier.value.trim())
            .find(|value| !value.is_empty())
            .map(str::to_owned)
            .or(fallback_document_id)
        else {
            eros::bail!("canonical document requires a stable document identifier")
        };

        Ok(Self::Document(Document {
            document_id,
            pdf_hash: None,
            title,
        }))
    }
}

impl TryFrom<&TeiDocument> for CanonicalDocument {
    type Error = eros::ErrorUnion;

    fn try_from(draft: &TeiDocument) -> Result<Self, Self::Error> {
        Self::try_from_draft(draft, None)
    }
}

impl CanonicalModel {
    /// Canonicalises a draft, using the exact PDF's SHA-256 as its identifier
    /// only when the draft has no usable bibliographic identifier.
    pub fn try_from_with_pdf_hash(draft: &TeiDocument, pdf_hash: &str) -> eros::Result<Self> {
        if pdf_hash.len() != 64
            || !pdf_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            eros::bail!("PDF hash must be a lowercase SHA-256 digest")
        }

        let fallback_document_id = format!("sha256:{pdf_hash}");
        let mut document = CanonicalDocument::try_from_draft(draft, Some(fallback_document_id))?;
        document.set_pdf_hash(pdf_hash.to_owned());
        Self::from_document_and_draft(document, draft)
    }

    fn from_document_and_draft(
        document: CanonicalDocument,
        draft: &TeiDocument,
    ) -> eros::Result<Self> {
        if draft.bibliography.authors.is_empty() {
            eros::bail!("canonical document requires at least one contributor")
        }

        let contributors = draft
            .bibliography
            .authors
            .iter()
            .enumerate()
            .map(|(index, contributor)| {
                let given_name = contributor
                    .forename
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned);
                let family_name = contributor
                    .surname
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .or_else(|| {
                        (given_name.is_none())
                            .then(|| contributor.name.trim())
                            .filter(|name| !name.is_empty())
                    })
                    .map(str::to_owned);

                if given_name.is_none() && family_name.is_none() {
                    eros::bail!("canonical contributor {} requires a name", index + 1)
                }

                Ok(CanonicalContributor {
                    person: Person {
                        person_id: format!("{}:contributor:{}", document.document_id(), index + 1),
                        given_name,
                        family_name,
                    },
                    contribution: match contributor.role {
                        ContributorRole::Author => CanonicalContribution::Authorship,
                        ContributorRole::Editor => CanonicalContribution::Contribution,
                    },
                })
            })
            .collect::<eros::Result<Vec<_>>>()?;

        let mut organizations = Vec::new();
        let mut organization_ids_by_name = HashMap::new();
        let mut affiliations = Vec::new();
        for (source, canonical) in draft.bibliography.authors.iter().zip(&contributors) {
            let Some(organization_name) = non_empty(source.affiliation.as_deref()) else {
                continue;
            };
            let normalized_name = normalize_name(organization_name);
            let organization_id = organization_ids_by_name
                .entry(normalized_name.clone())
                .or_insert_with(|| {
                    let organization_id =
                        scoped_id(document.document_id(), "organization", &normalized_name);
                    organizations.push(CanonicalOrganization::Organization(Organization {
                        organization_id: organization_id.clone(),
                        organization_name: organization_name.to_owned(),
                        ror_id: None,
                    }));
                    organization_id
                })
                .clone();
            affiliations.push(CanonicalAffiliation {
                person_id: canonical.person.person_id.clone(),
                organization_id,
                evidence_document_id: document.document_id().to_owned(),
            });
        }

        let mut publication_venues = Vec::new();
        let mut publication_events = Vec::new();
        if let Some(publication_date) = publication_datetime(draft) {
            let publisher_id = non_empty(draft.bibliography.publisher.as_deref()).map(|name| {
                let id = scoped_id(document.document_id(), "publisher", &normalize_name(name));
                organizations.push(CanonicalOrganization::Publisher(Publisher {
                    organization_id: id.clone(),
                    organization_name: name.to_owned(),
                    ror_id: None,
                }));
                id
            });
            let venue_id = non_empty(draft.bibliography.journal.as_deref()).map(|name| {
                let id = scoped_id(document.document_id(), "journal", &normalize_name(name));
                let issn = identifier(&draft.bibliography.identifiers, |kind| {
                    matches!(kind, IdentifierKind::Issn)
                })
                .map(str::to_owned);
                publication_venues.push(CanonicalPublicationVenue::Journal(Journal {
                    venue_id: id.clone(),
                    issn,
                    venue_name: name.to_owned(),
                }));
                id
            });
            publication_events.push(CanonicalPublicationEvent {
                kind: CanonicalPublicationEventKind::Publication,
                work_document_id: document.document_id().to_owned(),
                publisher_id,
                venue_id,
                publication_date,
                publication_notes: Vec::new(),
                version_number: None,
            });
        }

        Ok(Self {
            document,
            contributors,
            organizations,
            publication_venues,
            affiliations,
            publication_events,
        })
    }
}

impl TryFrom<&TeiDocument> for CanonicalModel {
    type Error = eros::ErrorUnion;

    fn try_from(draft: &TeiDocument) -> Result<Self, Self::Error> {
        let document = CanonicalDocument::try_from(draft)?;
        Self::from_document_and_draft(document, draft)
    }
}

fn identifier(
    identifiers: &[Identifier],
    matches_kind: impl Fn(&IdentifierKind) -> bool,
) -> Option<&str> {
    identifiers
        .iter()
        .find(|identifier| matches_kind(&identifier.kind) && !identifier.value.trim().is_empty())
        .map(|identifier| identifier.value.trim())
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn normalize_name(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn scoped_id(document_id: &str, kind: &str, value: &str) -> String {
    let digest = hex::encode(Sha256::digest(value.as_bytes()));
    format!("{document_id}:{kind}:{}", &digest[..16])
}

fn publication_datetime(draft: &TeiDocument) -> Option<NaiveDateTime> {
    if let Some(value) = non_empty(draft.bibliography.publication_date.as_deref()) {
        if let Ok(value) = DateTime::parse_from_rfc3339(value) {
            return Some(value.naive_utc());
        }
        for format in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"] {
            if let Ok(value) = NaiveDateTime::parse_from_str(value, format) {
                return Some(value);
            }
        }
        if let Ok(value) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
            return value.and_hms_opt(0, 0, 0);
        }
        if let Some((year, month)) = value
            .split_once('-')
            .and_then(|(year, month)| Some((year.parse().ok()?, month.parse().ok()?)))
        {
            return NaiveDate::from_ymd_opt(year, month, 1)
                .and_then(|date| date.and_hms_opt(0, 0, 0));
        }
        if let Ok(year) = value.parse() {
            return NaiveDate::from_ymd_opt(year, 1, 1).and_then(|date| date.and_hms_opt(0, 0, 0));
        }
    }
    draft
        .bibliography
        .publication_year
        .and_then(|year| NaiveDate::from_ymd_opt(i32::from(year), 1, 1))
        .and_then(|date| date.and_hms_opt(0, 0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::draft::{Bibliography, Contributor, IdentifierScope, PassageLevel};

    fn draft(title: Option<&str>, identifiers: Vec<Identifier>) -> TeiDocument {
        TeiDocument {
            level: PassageLevel::Paragraph,
            bibliography: Bibliography {
                title: title.map(str::to_owned),
                identifiers,
                authors: vec![Contributor {
                    name: "Ada Lovelace".to_owned(),
                    forename: Some("Ada".to_owned()),
                    surname: Some("Lovelace".to_owned()),
                    affiliation: None,
                    role: ContributorRole::Author,
                }],
                ..Bibliography::default()
            },
            body_text: vec![],
            figures_and_tables: vec![],
            references: vec![],
        }
    }

    fn id(kind: IdentifierKind, value: &str) -> Identifier {
        Identifier {
            kind,
            value: value.to_owned(),
            scope: IdentifierScope::Document,
        }
    }

    #[test]
    fn doi_draft_becomes_a_canonical_research_paper() {
        let canonical = CanonicalDocument::try_from(&draft(
            Some(" A paper "),
            vec![id(IdentifierKind::Doi, "10.1234/example")],
        ))
        .unwrap();

        assert_eq!(canonical.document_id(), "10.1234/example");
        assert_eq!(canonical.title(), "A paper");
        assert!(matches!(canonical, CanonicalDocument::ResearchPaper(_)));
    }

    #[test]
    fn draft_without_required_canonical_fields_is_rejected() {
        assert!(CanonicalDocument::try_from(&draft(None, vec![])).is_err());
        assert!(CanonicalDocument::try_from(&draft(Some("A paper"), vec![])).is_err());
    }

    #[test]
    fn canonical_model_contains_required_contribution() {
        let canonical = CanonicalModel::try_from(&draft(
            Some("A paper"),
            vec![id(IdentifierKind::Doi, "10.1234/example")],
        ))
        .unwrap();

        assert_eq!(canonical.contributors.len(), 1);
        assert_eq!(
            canonical.contributors[0].contribution,
            CanonicalContribution::Authorship
        );
    }

    #[test]
    fn canonical_model_contains_the_complete_persistable_graph() {
        let mut draft = draft(
            Some("A paper"),
            vec![
                id(IdentifierKind::Doi, "10.1234/example"),
                id(IdentifierKind::Issn, "1234-5678"),
            ],
        );
        draft.bibliography.authors[0].affiliation = Some("Example University".into());
        draft.bibliography.publisher = Some("Example Press".into());
        draft.bibliography.journal = Some("Example Journal".into());
        draft.bibliography.publication_date = Some("2024-05-06".into());
        draft.bibliography.publication_year = Some(2024);

        let canonical = CanonicalModel::try_from(&draft).unwrap();

        assert_eq!(canonical.organizations.len(), 2);
        assert_eq!(canonical.affiliations.len(), 1);
        assert_eq!(canonical.publication_venues.len(), 1);
        assert_eq!(canonical.publication_events.len(), 1);
        assert_eq!(
            canonical.affiliations[0].person_id,
            canonical.contributors[0].person.person_id
        );
        assert_eq!(
            canonical.affiliations[0].organization_id,
            canonical.organizations[0].organization_id()
        );
        assert_eq!(
            canonical.publication_events[0].publisher_id.as_deref(),
            Some(canonical.organizations[1].organization_id())
        );
        assert_eq!(canonical.publication_venues[0].issn(), Some("1234-5678"));
    }

    #[test]
    fn equal_affiliation_names_share_one_document_scoped_organization() {
        let mut draft = draft(
            Some("A paper"),
            vec![id(IdentifierKind::Doi, "10.1234/example")],
        );
        draft.bibliography.authors[0].affiliation = Some(" Example   University ".into());
        draft.bibliography.authors.push(Contributor {
            name: "Grace Hopper".into(),
            forename: Some("Grace".into()),
            surname: Some("Hopper".into()),
            affiliation: Some("example university".into()),
            role: ContributorRole::Author,
        });

        let canonical = CanonicalModel::try_from(&draft).unwrap();

        assert_eq!(canonical.organizations.len(), 1);
        assert_eq!(canonical.affiliations.len(), 2);
        assert_eq!(
            canonical.affiliations[0].organization_id,
            canonical.affiliations[1].organization_id
        );
    }
}
