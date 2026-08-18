use enum_dispatch::enum_dispatch;
use nonempty_collections::NEVec;
use std::sync::Arc;

use crate::models::canonical::relations::{
    affiliation::EAffiliation, contribution::EContribution, publication_event::EPublicationEvent,
};

#[enum_dispatch]
pub trait TDocument: Send + Sync {
    fn document_id(&self) -> &str;
    fn pdf_hash(&self) -> Option<&str>;
    fn title(&self) -> &str;
    fn entity_type(&self) -> &'static str;
    fn doi(&self) -> Option<&str> {
        None
    }
    fn isbn(&self) -> Option<&str> {
        None
    }
}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct Document {
    pub document_id: String,
    pub pdf_hash: Option<String>,
    pub title: String,
}

#[derive(bon::Builder)]
pub struct ADocument {
    pub document: Document,
    pub contributions: NEVec<Arc<EContribution>>,
    pub publication_events: Vec<Arc<EPublicationEvent>>,
    pub affiliations: Vec<Arc<EAffiliation>>,
}

impl TDocument for Document {
    fn document_id(&self) -> &str {
        &self.document_id
    }

    fn pdf_hash(&self) -> Option<&str> {
        self.pdf_hash.as_deref()
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn entity_type(&self) -> &'static str {
        "document"
    }
}

pub trait TResearchPaper: TDocument {
    fn doi(&self) -> Option<&str>;
}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct ResearchPaper {
    pub document_id: String,
    pub pdf_hash: Option<String>,
    pub title: String,
    pub doi: Option<String>,
}

#[derive(bon::Builder)]
pub struct AResearchPaper {
    pub research_paper: ResearchPaper,
    pub contributions: NEVec<Arc<EContribution>>,
    pub publication_events: Vec<Arc<EPublicationEvent>>,
    pub affiliations: Vec<Arc<EAffiliation>>,
}

impl TDocument for ResearchPaper {
    fn document_id(&self) -> &str {
        &self.document_id
    }

    fn pdf_hash(&self) -> Option<&str> {
        self.pdf_hash.as_deref()
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn entity_type(&self) -> &'static str {
        "research_paper"
    }

    fn doi(&self) -> Option<&str> {
        self.doi.as_deref()
    }
}
impl TResearchPaper for ResearchPaper {
    fn doi(&self) -> Option<&str> {
        self.doi.as_deref()
    }
}

pub trait TBook: TDocument {
    fn isbn(&self) -> Option<&str>;
}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct Book {
    pub document_id: String,
    pub pdf_hash: Option<String>,
    pub title: String,
    pub isbn: Option<String>,
}

#[derive(bon::Builder)]
pub struct ABook {
    pub book: Book,
    pub contributions: NEVec<Arc<EContribution>>,
    pub publication_events: Vec<Arc<EPublicationEvent>>,
    pub affiliations: Vec<Arc<EAffiliation>>,
}

impl TDocument for Book {
    fn document_id(&self) -> &str {
        &self.document_id
    }

    fn pdf_hash(&self) -> Option<&str> {
        self.pdf_hash.as_deref()
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn entity_type(&self) -> &'static str {
        "book"
    }

    fn isbn(&self) -> Option<&str> {
        self.isbn.as_deref()
    }
}
impl TBook for Book {
    fn isbn(&self) -> Option<&str> {
        self.isbn.as_deref()
    }
}

pub trait TReport: TDocument {}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct Report {
    pub document_id: String,
    pub pdf_hash: Option<String>,
    pub title: String,
}

#[derive(bon::Builder)]
pub struct AReport {
    pub report: Report,
    pub contributions: NEVec<Arc<EContribution>>,
    pub publication_events: Vec<Arc<EPublicationEvent>>,
    pub affiliations: Vec<Arc<EAffiliation>>,
}

impl TDocument for Report {
    fn document_id(&self) -> &str {
        &self.document_id
    }

    fn pdf_hash(&self) -> Option<&str> {
        self.pdf_hash.as_deref()
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn entity_type(&self) -> &'static str {
        "report"
    }
}
impl TReport for Report {}

#[enum_dispatch(TDocument)]
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EDocument {
    Document,
    ResearchPaper,
    Book,
    Report,
}
