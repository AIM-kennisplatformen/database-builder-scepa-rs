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
    Document,
    ResearchPaper,
    Report,
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
    Any,
    Publisher,
    Affiliation,
    Contributor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationTypeFilter {
    Organization,
    Institution,
    GovernmentInstitution,
    EducationalInstitution,
    NonprofitInstitution,
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
    pub doi: Option<String>,
    pub isbn: Vec<String>,
    pub persons: Vec<PartyMetadata>,
    pub organizations: Vec<PartyMetadata>,
    pub contributors: Vec<ContributionMetadata>,
    pub affiliations: Vec<AffiliationMetadata>,
    pub publication_events: Vec<PublicationEventMetadata>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CombinedPassageCandidate {
    pub point_id: String,
    pub pdf_hash: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LiteratureResult {
    pub text: String,
    pub pdf_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LiteratureSearchResponse {
    pub results: Vec<LiteratureResult>,
    pub usage_note: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_by_pdf_hash: Option<BTreeMap<String, DocumentMetadata>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MetadataResponse {
    pub documents: BTreeMap<String, DocumentMetadata>,
    pub not_found: Vec<String>,
}
