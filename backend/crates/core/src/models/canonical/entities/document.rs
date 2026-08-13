use crate::models::canonical::relations::{affiliation, contribution, publication_event};
use nonempty_collections::NEVec;
use std::rc::Rc;

pub trait TDocument {
    fn document_id(&self) -> &str;
    fn pdf_hash(&self) -> Option<&str>;
    fn title(&self) -> &str;
}

impl<T: TDocument + ?Sized> contribution::TWork for T {}
impl<T: TDocument + ?Sized> publication_event::TWork for T {}
impl<T: TDocument + ?Sized> affiliation::TEvidence for T {}

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
    pub contributions: NEVec<Rc<dyn contribution::TContribution>>,
    pub publication_events: Vec<Rc<dyn publication_event::TPublicationEvent>>,
    pub affiliations: Vec<Rc<dyn affiliation::TAffiliation>>,
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
    pub contributions: NEVec<Rc<dyn contribution::TContribution>>,
    pub publication_events: Vec<Rc<dyn publication_event::TPublicationEvent>>,
    pub affiliations: Vec<Rc<dyn affiliation::TAffiliation>>,
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
    pub contributions: NEVec<Rc<dyn contribution::TContribution>>,
    pub publication_events: Vec<Rc<dyn publication_event::TPublicationEvent>>,
    pub affiliations: Vec<Rc<dyn affiliation::TAffiliation>>,
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
}
impl TBook for Book {
    fn isbn(&self) -> Option<&str> {
        self.isbn.as_deref()
    }
}

pub trait TReport: TDocument {}

#[derive(bon::Builder)]
#[builder(on(String, into))]
pub struct Report {
    pub document_id: String,
    pub pdf_hash: Option<String>,
    pub title: String,
}

#[derive(bon::Builder)]
pub struct AReport {
    pub report: Report,
    pub contributions: NEVec<Rc<dyn contribution::TContribution>>,
    pub publication_events: Vec<Rc<dyn publication_event::TPublicationEvent>>,
    pub affiliations: Vec<Rc<dyn affiliation::TAffiliation>>,
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
}
impl TReport for Report {}
