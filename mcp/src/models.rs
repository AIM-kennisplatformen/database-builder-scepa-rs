use std::collections::BTreeMap;

use chrono::NaiveDate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublicationDateFilter {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DocumentTypeFilter {
    /// The exact base document type, excluding its research_paper, report, and book subtypes.
    Document,
    /// A scholarly research paper.
    ResearchPaper,
    /// A report.
    Report,
    /// A book.
    Book,
}

impl DocumentTypeFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::ResearchPaper => "research_paper",
            Self::Report => "report",
            Self::Book => "book",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationRoleFilter {
    /// Match publisher, affiliation, or contributor relationships.
    Any,
    /// An organization that published the document.
    Publisher,
    /// An organization affiliated with a person evidenced by the document.
    Affiliation,
    /// An organization recorded as a direct contributor to the document.
    Contributor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationTypeFilter {
    /// Any organization, including all organization subtypes.
    Organization,
    /// Any institution, including government, educational, and nonprofit institutions.
    Institution,
    /// A government institution.
    GovernmentInstitution,
    /// An educational institution.
    EducationalInstitution,
    /// A nonprofit institution.
    NonprofitInstitution,
    /// A publishing organization.
    Publisher,
}

impl OrganizationTypeFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::Organization => "organization",
            Self::Institution => "institution",
            Self::GovernmentInstitution => "government_institution",
            Self::EducationalInstitution => "educational_institution",
            Self::NonprofitInstitution => "nonprofit_institution",
            Self::Publisher => "publisher",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OrganizationFilter {
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub roles: Vec<OrganizationRoleFilter>,
    #[serde(default)]
    pub types: Vec<OrganizationTypeFilter>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LiteratureFilters {
    pub publication_date: Option<PublicationDateFilter>,
    #[serde(default)]
    pub document_types: Vec<DocumentTypeFilter>,
    pub organization: Option<OrganizationFilter>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PartyMetadata {
    pub id: String,
    pub entity_type: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub organization_name: Option<String>,
    pub ror_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContributionMetadata {
    pub contribution_type: String,
    pub contributor: PartyMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AffiliationMetadata {
    pub person_id: String,
    pub organization_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VenueMetadata {
    pub id: String,
    pub venue_type: String,
    pub name: String,
    pub issn: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublicationEventMetadata {
    pub event_type: String,
    pub publication_date: String,
    pub publication_notes: Vec<String>,
    pub version_number: Option<String>,
    pub publisher: Option<PartyMetadata>,
    pub venue: Option<VenueMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DocumentMetadata {
    pub pdf_hash: String,
    pub document_id: String,
    pub document_type: String,
    pub title: String,
    /// IEEE-style reference text derived only from the metadata in this response.
    pub ieee_reference: String,
    pub doi: Option<String>,
    pub isbn: Vec<String>,
    pub persons: Vec<PartyMetadata>,
    pub organizations: Vec<PartyMetadata>,
    pub contributors: Vec<ContributionMetadata>,
    pub affiliations: Vec<AffiliationMetadata>,
    pub publication_events: Vec<PublicationEventMetadata>,
}

impl DocumentMetadata {
    pub fn refresh_ieee_reference(&mut self) {
        let mut authors = self
            .contributors
            .iter()
            .filter(|contribution| contribution.contribution_type == "authorship")
            .filter_map(|contribution| {
                ieee_author(&contribution.contributor)
                    .map(|author| (contribution.contributor.id.clone(), author))
            })
            .collect::<Vec<_>>();
        authors.sort_by(|left, right| left.0.cmp(&right.0));
        let mut authors = authors
            .into_iter()
            .map(|(_, author)| author)
            .collect::<Vec<_>>();
        authors.dedup();
        if authors.len() > 6 {
            authors.truncate(6);
            authors.push("et al.".into());
        }

        let publication = self
            .publication_events
            .iter()
            .find(|event| event.event_type == "publication")
            .or_else(|| self.publication_events.first());
        let mut parts = Vec::new();
        if !authors.is_empty() {
            parts.push(authors.join(", "));
        }
        parts.push(format!("\"{}\"", self.title.replace('"', "'")));
        if let Some(venue) = publication.and_then(|event| event.venue.as_ref()) {
            parts.push(venue.name.clone());
        } else if let Some(publisher) = publication
            .and_then(|event| event.publisher.as_ref())
            .and_then(|party| party.organization_name.as_ref())
        {
            parts.push(publisher.clone());
        }
        if let Some(year) = publication
            .and_then(|event| event.publication_date.get(..4))
            .filter(|year| year.chars().all(|character| character.is_ascii_digit()))
        {
            parts.push(year.into());
        }
        if let Some(doi) = &self.doi {
            parts.push(format!("doi: {doi}"));
        }
        self.ieee_reference = format!("{}.", parts.join(", "));
    }
}

fn ieee_author(party: &PartyMetadata) -> Option<String> {
    if party.entity_type != "person" {
        return None;
    }
    let family_name = party.family_name.as_deref()?.trim();
    if family_name.is_empty() {
        return None;
    }
    let given_name = party.given_name.as_deref()?.trim();
    if given_name.is_empty() {
        return None;
    }
    let initials = given_name
        .split(|character: char| character.is_whitespace() || character == '-')
        .filter_map(|name| name.chars().next())
        .map(|initial| format!("{initial}."))
        .collect::<Vec<_>>()
        .join(" ");
    Some(format!("{initials} {family_name}"))
}

#[derive(Clone, Debug, PartialEq)]
pub struct CombinedPassageCandidate {
    pub point_id: String,
    pub pdf_hash: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LiteratureResult {
    pub text: String,
    pub pdf_hash: String,
    /// Normalized reranker relevance score used internally and never shown to the user.
    pub score: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LiteratureSearchResponse {
    pub results: Vec<LiteratureResult>,
    pub usage_note: String,
    pub metadata_by_pdf_hash: BTreeMap<String, DocumentMetadata>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(id: &str, given_name: &str, family_name: &str) -> PartyMetadata {
        PartyMetadata {
            id: id.into(),
            entity_type: "person".into(),
            given_name: Some(given_name.into()),
            family_name: Some(family_name.into()),
            organization_name: None,
            ror_id: None,
        }
    }

    #[test]
    fn ieee_reference_uses_only_available_document_metadata() {
        let mut document = DocumentMetadata {
            pdf_hash: "opaque".into(),
            document_id: "10.1000/example".into(),
            document_type: "research_paper".into(),
            title: "Grounded Evidence".into(),
            ieee_reference: String::new(),
            doi: Some("10.1000/example".into()),
            isbn: Vec::new(),
            persons: Vec::new(),
            organizations: Vec::new(),
            contributors: vec![
                ContributionMetadata {
                    contribution_type: "authorship".into(),
                    contributor: person("contributor:2", "Bea", "Beta"),
                },
                ContributionMetadata {
                    contribution_type: "authorship".into(),
                    contributor: person("contributor:1", "Ada Maria", "Alpha"),
                },
            ],
            affiliations: Vec::new(),
            publication_events: vec![PublicationEventMetadata {
                event_type: "publication".into(),
                publication_date: "2024-06-15T00:00:00".into(),
                publication_notes: Vec::new(),
                version_number: None,
                publisher: None,
                venue: Some(VenueMetadata {
                    id: "venue:1".into(),
                    venue_type: "journal".into(),
                    name: "Journal of Tests".into(),
                    issn: None,
                }),
            }],
        };

        document.refresh_ieee_reference();

        assert_eq!(
            document.ieee_reference,
            "A. M. Alpha, B. Beta, \"Grounded Evidence\", Journal of Tests, 2024, doi: 10.1000/example."
        );
        assert!(!document.ieee_reference.contains("opaque"));
    }

    #[test]
    fn ieee_reference_omits_unavailable_fields_and_non_person_contributors() {
        let mut document = DocumentMetadata {
            pdf_hash: "opaque".into(),
            document_id: "internal".into(),
            document_type: "report".into(),
            title: "Minimal Report".into(),
            ieee_reference: String::new(),
            doi: None,
            isbn: Vec::new(),
            persons: Vec::new(),
            organizations: Vec::new(),
            contributors: vec![ContributionMetadata {
                contribution_type: "authorship".into(),
                contributor: PartyMetadata {
                    id: "malformed-person:1".into(),
                    entity_type: "person".into(),
                    given_name: None,
                    family_name: Some("Extraction artifact with no given name".into()),
                    organization_name: None,
                    ror_id: None,
                },
            }],
            affiliations: Vec::new(),
            publication_events: Vec::new(),
        };

        document.refresh_ieee_reference();

        assert_eq!(document.ieee_reference, "\"Minimal Report\".");
    }

    #[test]
    fn literature_search_response_serializes_scores_and_metadata_map() {
        let response = LiteratureSearchResponse {
            results: vec![LiteratureResult {
                text: "Evidence passage".into(),
                pdf_hash: "opaque".into(),
                score: 0.875,
            }],
            usage_note: "opaque key".into(),
            metadata_by_pdf_hash: BTreeMap::new(),
        };

        let json = serde_json::to_value(response).expect("response should serialize");
        assert_eq!(json["results"][0]["score"], serde_json::json!(0.875));
        assert_eq!(
            json.get("metadata_by_pdf_hash"),
            Some(&serde_json::json!({}))
        );
    }
}
