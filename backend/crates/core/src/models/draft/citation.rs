use serde::{Deserialize, Serialize};

use crate::models::draft::bibliography::{Contributor, Identifier};

/// A bibliography entry from `listBibl`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema)]
#[builder(on(String, into))]
pub struct Citation {
    pub id: String,
    pub target: Option<String>,
    pub title: Option<String>,
    pub contributors: Vec<Contributor>,
    pub publication: PublicationMetadata,
    pub identifiers: Vec<Identifier>,
    pub reference_text: Option<String>,
    pub raw_reference: Option<String>,
    pub notes: Vec<CitationNote>,
    pub urls: Vec<String>,
}

#[derive(
    Clone, Debug, Default, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct PublicationMetadata {
    pub journal: Option<String>,
    pub series: Option<String>,
    pub publisher: Option<String>,
    pub publisher_location: Option<String>,
    pub publication_date: Option<String>,
    pub year: Option<u16>,
    pub page_start: Option<String>,
    pub page_end: Option<String>,
    pub pages: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub chapter: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema)]
#[builder(on(String, into))]
pub struct CitationNote {
    pub kind: Option<String>,
    pub text: String,
}
