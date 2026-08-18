//! Root canonical graph and conversion from the extraction draft.

use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, NaiveDate, NaiveDateTime};
use nonempty_collections::NEVec;
use sha2::{Digest, Sha256};

use super::{
    entities::{
        document::{Book, Document, EDocument, ResearchPaper, TDocument},
        organization::{EOrganization, Organization, Publisher},
        person::{EPerson, Person},
        publication_venue::{EPublicationVenue, Journal},
    },
    relations::{
        affiliation::{Affiliation, EAffiliation},
        contribution::{Authorship, Contribution, EContribution, EContributor},
        publication_event::{EPublicationEvent, Publication},
    },
};
use crate::models::draft::{ContributorRole, Identifier, IdentifierKind, TeiDocument};

/// A canonical object graph whose nodes and relations map directly to the TypeDB schema.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct CanonicalModel {
    pub document: Arc<EDocument>,
    pub persons: Vec<Arc<EPerson>>,
    pub organizations: Vec<Arc<EOrganization>>,
    pub publication_venues: Vec<Arc<EPublicationVenue>>,
    pub contributions: Vec<Arc<EContribution>>,
    pub affiliations: Vec<Arc<EAffiliation>>,
    pub publication_events: Vec<Arc<EPublicationEvent>>,
}

struct DocumentNode {
    entity: Arc<EDocument>,
    contribution_work: Arc<EDocument>,
    publication_work: Arc<EDocument>,
    evidence: Arc<EDocument>,
}

fn document_node<T>(document: T) -> DocumentNode
where
    T: Into<EDocument>,
{
    let document = Arc::new(document.into());
    DocumentNode {
        entity: document.clone(),
        contribution_work: document.clone(),
        publication_work: document.clone(),
        evidence: document,
    }
}

struct OrganizationNode {
    entity: Arc<EOrganization>,
    affiliation: Arc<EOrganization>,
    publisher: Arc<EOrganization>,
}

fn organization_node<T>(organization: T) -> OrganizationNode
where
    T: Into<EOrganization>,
{
    let organization = Arc::new(organization.into());
    OrganizationNode {
        entity: organization.clone(),
        affiliation: organization.clone(),
        publisher: organization,
    }
}

struct VenueNode {
    entity: Arc<EPublicationVenue>,
    publication_venue: Arc<EPublicationVenue>,
}

fn venue_node<T>(venue: T) -> VenueNode
where
    T: Into<EPublicationVenue>,
{
    let venue = Arc::new(venue.into());
    VenueNode {
        entity: venue.clone(),
        publication_venue: venue,
    }
}

impl CanonicalModel {
    /// Canonicalises a draft, using the exact PDF SHA-256 as its identifier
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
        Self::from_draft(draft, Some(fallback_document_id), Some(pdf_hash.to_owned()))
    }

    fn from_draft(
        draft: &TeiDocument,
        fallback_document_id: Option<String>,
        pdf_hash: Option<String>,
    ) -> eros::Result<Self> {
        let document = canonical_document(draft, fallback_document_id, pdf_hash)?;
        if draft.bibliography.authors.is_empty() {
            eros::bail!("canonical document requires at least one contributor")
        }

        let mut persons: Vec<Arc<EPerson>> = Vec::new();
        let mut person_affiliates: Vec<Arc<EPerson>> = Vec::new();
        let mut contributions: Vec<Arc<EContribution>> = Vec::new();

        for (index, contributor) in draft.bibliography.authors.iter().enumerate() {
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

            let person = Arc::new(EPerson::from(Person {
                person_id: format!(
                    "{}:contributor:{}",
                    document.entity.document_id(),
                    index + 1
                ),
                given_name,
                family_name,
            }));
            let contributor_role = Arc::new(EContributor::Person(person.clone()));
            let relation = match contributor.role {
                ContributorRole::Author => Arc::new(EContribution::from(Authorship {
                    contributor: contributor_role.clone(),
                    work: document.contribution_work.clone(),
                })),
                ContributorRole::Editor => Arc::new(EContribution::from(Contribution {
                    contributor: contributor_role.clone(),
                    work: document.contribution_work.clone(),
                })),
            };
            persons.push(person.clone());
            person_affiliates.push(person);
            contributions.push(relation);
        }

        let mut organizations: Vec<Arc<EOrganization>> = Vec::new();
        let mut affiliation_organizations: HashMap<String, Arc<EOrganization>> = HashMap::new();
        let mut affiliations: Vec<Arc<EAffiliation>> = Vec::new();

        for (source, person) in draft.bibliography.authors.iter().zip(&person_affiliates) {
            let Some(organization_name) = non_empty(source.affiliation.as_deref()) else {
                continue;
            };
            let normalized_name = normalize_name(organization_name);
            let organization =
                if let Some(existing) = affiliation_organizations.get(&normalized_name) {
                    existing.clone()
                } else {
                    let organization_id = scoped_id(
                        document.entity.document_id(),
                        "organization",
                        &normalized_name,
                    );
                    let node = organization_node(Organization {
                        organization_id,
                        organization_name: organization_name.to_owned(),
                        ror_id: None,
                    });
                    organizations.push(node.entity);
                    affiliation_organizations.insert(normalized_name, node.affiliation.clone());
                    node.affiliation
                };
            affiliations.push(Arc::new(EAffiliation::from(Affiliation {
                person: person.clone(),
                organization,
                evidence: NEVec::new(document.evidence.clone()),
            })));
        }

        let mut publication_venues: Vec<Arc<EPublicationVenue>> = Vec::new();
        let mut publication_events: Vec<Arc<EPublicationEvent>> = Vec::new();
        if let Some(publication_date) = publication_datetime(draft) {
            let publisher = non_empty(draft.bibliography.publisher.as_deref()).map(|name| {
                let node = organization_node(Publisher {
                    organization_id: scoped_id(
                        document.entity.document_id(),
                        "publisher",
                        &normalize_name(name),
                    ),
                    organization_name: name.to_owned(),
                    ror_id: None,
                });
                organizations.push(node.entity);
                node.publisher
            });
            let venue = non_empty(draft.bibliography.journal.as_deref()).map(|name| {
                let node = venue_node(Journal {
                    venue_id: scoped_id(
                        document.entity.document_id(),
                        "journal",
                        &normalize_name(name),
                    ),
                    issn: identifier(&draft.bibliography.identifiers, |kind| {
                        matches!(kind, IdentifierKind::Issn)
                    })
                    .map(str::to_owned),
                    venue_name: name.to_owned(),
                });
                publication_venues.push(node.entity);
                node.publication_venue
            });
            publication_events.push(Arc::new(EPublicationEvent::from(Publication {
                publisher,
                venue,
                work: document.publication_work.clone(),
                publication_date,
                publication_notes: Vec::new(),
                version_number: None,
            })));
        }

        Ok(Self {
            document: document.entity,
            persons,
            organizations,
            publication_venues,
            contributions,
            affiliations,
            publication_events,
        })
    }
}

impl TryFrom<&TeiDocument> for CanonicalModel {
    type Error = eros::ErrorUnion;

    fn try_from(draft: &TeiDocument) -> Result<Self, Self::Error> {
        Self::from_draft(draft, None, None)
    }
}

fn canonical_document(
    draft: &TeiDocument,
    fallback_document_id: Option<String>,
    pdf_hash: Option<String>,
) -> eros::Result<DocumentNode> {
    let Some(title) = non_empty(draft.bibliography.title.as_deref()) else {
        eros::bail!("canonical document requires a title")
    };
    let doi = identifier(&draft.bibliography.identifiers, |kind| {
        matches!(kind, IdentifierKind::Doi)
    });
    if let Some(doi) = doi {
        return Ok(document_node(ResearchPaper {
            document_id: doi.to_owned(),
            pdf_hash,
            title: title.to_owned(),
            doi: Some(doi.to_owned()),
        }));
    }
    let isbn = identifier(&draft.bibliography.identifiers, |kind| {
        matches!(kind, IdentifierKind::Isbn)
    });
    if let Some(isbn) = isbn {
        return Ok(document_node(Book {
            document_id: isbn.to_owned(),
            pdf_hash,
            title: title.to_owned(),
            isbn: Some(isbn.to_owned()),
        }));
    }
    let Some(document_id) = draft
        .bibliography
        .identifiers
        .iter()
        .filter(|identifier| !matches!(identifier.kind, IdentifierKind::Issn | IdentifierKind::Md5))
        .map(|identifier| identifier.value.trim())
        .find(|value| !value.is_empty())
        .map(str::to_owned)
        .or(fallback_document_id)
    else {
        eros::bail!("canonical document requires a stable document identifier")
    };
    Ok(document_node(Document {
        document_id,
        pdf_hash,
        title: title.to_owned(),
    }))
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
    use crate::models::canonical::{
        entities::{organization::TOrganization, person::TPerson},
        relations::{affiliation::TAffiliation, contribution::TContribution},
    };
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
        let canonical = CanonicalModel::try_from(&draft(
            Some(" A paper "),
            vec![id(IdentifierKind::Doi, "10.1234/example")],
        ))
        .unwrap();
        assert_eq!(canonical.document.document_id(), "10.1234/example");
        assert_eq!(canonical.document.title(), "A paper");
        assert_eq!(canonical.document.entity_type(), "research_paper");
    }

    #[test]
    fn draft_without_required_canonical_fields_is_rejected() {
        assert!(CanonicalModel::try_from(&draft(None, vec![])).is_err());
        assert!(CanonicalModel::try_from(&draft(Some("A paper"), vec![])).is_err());
    }

    #[test]
    fn canonical_model_contains_required_contribution() {
        let canonical = CanonicalModel::try_from(&draft(
            Some("A paper"),
            vec![id(IdentifierKind::Doi, "10.1234/example")],
        ))
        .unwrap();
        assert_eq!(canonical.persons.len(), 1);
        assert_eq!(canonical.contributions.len(), 1);
        assert_eq!(canonical.contributions[0].relation_type(), "authorship");
    }

    #[test]
    fn canonical_graph_round_trips_through_enums() {
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
        let canonical = CanonicalModel::try_from(&draft).unwrap();

        let json = serde_json::to_value(&canonical).unwrap();
        assert_eq!(json["document"]["type"], "research_paper");
        assert_eq!(json["persons"][0]["type"], "person");
        assert_eq!(json["organizations"][0]["type"], "organization");
        assert_eq!(json["publication_venues"][0]["type"], "journal");
        assert_eq!(json["contributions"][0]["type"], "authorship");
        assert_eq!(json["affiliations"][0]["type"], "affiliation");
        assert_eq!(json["publication_events"][0]["type"], "publication");
        let decoded: CanonicalModel = serde_json::from_value(json).unwrap();

        assert_eq!(decoded, canonical);
        assert_eq!(decoded.organizations.len(), 2);
        assert_eq!(decoded.affiliations.len(), 1);
        assert_eq!(decoded.publication_venues.len(), 1);
        assert_eq!(decoded.publication_events.len(), 1);
        assert_eq!(
            decoded.affiliations[0].person().person_id(),
            decoded.persons[0].person_id()
        );
    }

    #[test]
    fn canonical_graph_can_cross_async_send_sync_boundaries() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CanonicalModel>();
    }

    #[test]
    fn equal_affiliation_names_share_one_organization_before_serialization() {
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
            canonical.affiliations[0].organization().organization_id(),
            canonical.affiliations[1].organization().organization_id()
        );
    }
}
