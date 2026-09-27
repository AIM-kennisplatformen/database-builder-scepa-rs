use serde::{Deserialize, Serialize};

use crate::models::draft::passage::TextPassage;

/// Bibliographic data describing the converted document itself.
#[derive(
    Clone, Debug, Default, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct Bibliography {
    pub title: Option<String>,
    pub authors: Vec<Contributor>,
    pub identifiers: Vec<Identifier>,
    pub publication_date: Option<String>,
    pub publication_year: Option<u16>,
    pub publisher: Option<DraftOrganization>,
    pub journal: Option<DraftPublicationVenue>,
    pub journal_abbreviation: Option<String>,
    pub publication_event_id: Option<String>,
    pub abstract_text: Vec<TextPassage>,
}

/// A person credited by a document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema)]
#[builder(on(String, into))]
pub struct Contributor {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub contribution_id: String,
    pub name: String,
    pub forename: Option<String>,
    pub surname: Option<String>,
    pub affiliation: Option<DraftAffiliation>,
    pub role: ContributorRole,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema)]
#[builder(on(String, into))]
pub struct DraftOrganization {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub ror_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema)]
#[builder(on(String, into))]
pub struct DraftPublicationVenue {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub abbreviation: Option<String>,
    pub issn: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema)]
pub struct DraftAffiliation {
    #[serde(default)]
    pub id: String,
    pub organization: DraftOrganization,
}

impl From<&str> for DraftOrganization {
    fn from(name: &str) -> Self {
        Self {
            id: String::new(),
            name: name.into(),
            ror_id: None,
        }
    }
}

impl From<&str> for DraftPublicationVenue {
    fn from(name: &str) -> Self {
        Self {
            id: String::new(),
            name: name.into(),
            abbreviation: None,
            issn: None,
        }
    }
}

impl From<&str> for DraftAffiliation {
    fn from(name: &str) -> Self {
        Self {
            id: String::new(),
            organization: DraftOrganization::from(name),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContributorRole {
    Author,
    Editor,
}

/// A typed identifier together with the TEI level on which it occurred.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema)]
#[builder(on(String, into))]
pub struct Identifier {
    pub kind: IdentifierKind,
    pub value: String,
    pub scope: IdentifierScope,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IdentifierKind {
    Doi,
    Isbn,
    Issn,
    Pmc,
    Pmid,
    Arxiv,
    Md5,
    Other(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IdentifierScope {
    Document,
    Analytic,
    Monograph,
}
