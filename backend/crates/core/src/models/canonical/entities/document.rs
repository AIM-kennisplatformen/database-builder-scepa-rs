use crate::models::canonical::relations::{affiliation, contribution, publication_event};
use nonempty_collections::NEVec;
use std::sync::Arc;

#[typetag::serde(tag = "type")]
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

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct Document {
    pub document_id: String,
    pub pdf_hash: Option<String>,
    pub title: String,
}

#[derive(bon::Builder)]
pub struct ADocument {
    pub document: Document,
    pub contributions: NEVec<Arc<dyn contribution::TContribution>>,
    pub publication_events: Vec<Arc<dyn publication_event::TPublicationEvent>>,
    pub affiliations: Vec<Arc<dyn affiliation::TAffiliation>>,
}

#[typetag::serde(name = "document")]
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

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder)]
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
    pub contributions: NEVec<Arc<dyn contribution::TContribution>>,
    pub publication_events: Vec<Arc<dyn publication_event::TPublicationEvent>>,
    pub affiliations: Vec<Arc<dyn affiliation::TAffiliation>>,
}

#[typetag::serde(name = "research_paper")]
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

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder)]
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
    pub contributions: NEVec<Arc<dyn contribution::TContribution>>,
    pub publication_events: Vec<Arc<dyn publication_event::TPublicationEvent>>,
    pub affiliations: Vec<Arc<dyn affiliation::TAffiliation>>,
}

#[typetag::serde(name = "book")]
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

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct Report {
    pub document_id: String,
    pub pdf_hash: Option<String>,
    pub title: String,
}

#[derive(bon::Builder)]
pub struct AReport {
    pub report: Report,
    pub contributions: NEVec<Arc<dyn contribution::TContribution>>,
    pub publication_events: Vec<Arc<dyn publication_event::TPublicationEvent>>,
    pub affiliations: Vec<Arc<dyn affiliation::TAffiliation>>,
}

#[typetag::serde(name = "report")]
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

macro_rules! impl_document_roles {
    ($type:ty, $name:literal) => {
        #[typetag::serde(name = $name)]
        impl contribution::TWork for $type {
            fn document_id(&self) -> &str {
                TDocument::document_id(self)
            }
        }

        #[typetag::serde(name = $name)]
        impl publication_event::TWork for $type {
            fn document_id(&self) -> &str {
                TDocument::document_id(self)
            }
        }

        #[typetag::serde(name = $name)]
        impl affiliation::TEvidence for $type {
            fn document_id(&self) -> &str {
                TDocument::document_id(self)
            }
        }
    };
}

impl_document_roles!(Document, "document");
impl_document_roles!(ResearchPaper, "research_paper");
impl_document_roles!(Book, "book");
impl_document_roles!(Report, "report");
