use serde::{Deserialize, Serialize};

use crate::models::draft::passage::TextPassage;

/// Bibliographic data describing the converted document itself.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct Bibliography {
    pub title: Option<String>,
    pub authors: Vec<Contributor>,
    pub identifiers: Vec<Identifier>,
    pub publication_date: Option<String>,
    pub publication_year: Option<u16>,
    pub publisher: Option<String>,
    pub journal: Option<String>,
    pub journal_abbreviation: Option<String>,
    pub abstract_text: Vec<TextPassage>,
}

/// A person credited by a document or citation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct Contributor {
    pub name: String,
    pub forename: Option<String>,
    pub surname: Option<String>,
    pub affiliation: Option<String>,
    pub role: ContributorRole,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributorRole {
    Author,
    Editor,
}

/// A typed identifier together with the TEI level on which it occurred.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct Identifier {
    pub kind: IdentifierKind,
    pub value: String,
    pub scope: IdentifierScope,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentifierScope {
    Document,
    Analytic,
    Monograph,
    Citation,
}
