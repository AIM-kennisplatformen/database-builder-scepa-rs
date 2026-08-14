//! The abstract `publication_event` relation and its descendants.

use chrono::NaiveDateTime;
use std::sync::Arc;

#[typetag::serde(tag = "type")]
pub trait TPublisher: Send + Sync {
    fn organization_id(&self) -> &str;
}
#[typetag::serde(tag = "type")]
pub trait TPublicationVenue: Send + Sync {
    fn venue_id(&self) -> &str;
}
#[typetag::serde(tag = "type")]
pub trait TWork: Send + Sync {
    fn document_id(&self) -> &str;
}

/// Common roles and attributes of every publication event.
#[typetag::serde(tag = "type")]
pub trait TPublicationEvent: Send + Sync {
    fn publisher(&self) -> Option<&Arc<dyn TPublisher>>;
    fn venue(&self) -> Option<&Arc<dyn TPublicationVenue>>;
    fn work(&self) -> &Arc<dyn TWork>;
    fn publication_date(&self) -> NaiveDateTime;
    fn publication_notes(&self) -> &[String];
    fn version_number(&self) -> Option<&str> {
        None
    }
    fn relation_type(&self) -> &'static str;
}

pub trait TSubmission: TPublicationEvent {}

#[derive(serde::Serialize, serde::Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct Submission {
    pub publisher: Option<Arc<dyn TPublisher>>,
    pub venue: Option<Arc<dyn TPublicationVenue>>,
    pub work: Arc<dyn TWork>,
    pub publication_date: NaiveDateTime,
    pub publication_notes: Vec<String>,
}

impl TSubmission for Submission {}
#[typetag::serde(name = "submission")]
impl TPublicationEvent for Submission {
    fn publisher(&self) -> Option<&Arc<dyn TPublisher>> {
        self.publisher.as_ref()
    }

    fn venue(&self) -> Option<&Arc<dyn TPublicationVenue>> {
        self.venue.as_ref()
    }

    fn work(&self) -> &Arc<dyn TWork> {
        &self.work
    }

    fn publication_date(&self) -> NaiveDateTime {
        self.publication_date
    }

    fn publication_notes(&self) -> &[String] {
        &self.publication_notes
    }

    fn relation_type(&self) -> &'static str {
        "submission"
    }
}

pub trait TAcceptance: TPublicationEvent {}

#[derive(serde::Serialize, serde::Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct Acceptance {
    pub publisher: Option<Arc<dyn TPublisher>>,
    pub venue: Option<Arc<dyn TPublicationVenue>>,
    pub work: Arc<dyn TWork>,
    pub publication_date: NaiveDateTime,
    pub publication_notes: Vec<String>,
}

impl TAcceptance for Acceptance {}
#[typetag::serde(name = "acceptance")]
impl TPublicationEvent for Acceptance {
    fn publisher(&self) -> Option<&Arc<dyn TPublisher>> {
        self.publisher.as_ref()
    }

    fn venue(&self) -> Option<&Arc<dyn TPublicationVenue>> {
        self.venue.as_ref()
    }

    fn work(&self) -> &Arc<dyn TWork> {
        &self.work
    }

    fn publication_date(&self) -> NaiveDateTime {
        self.publication_date
    }

    fn publication_notes(&self) -> &[String] {
        &self.publication_notes
    }

    fn relation_type(&self) -> &'static str {
        "acceptance"
    }
}

pub trait TPublication: TPublicationEvent {
    fn version_number(&self) -> Option<&str>;
}

#[derive(serde::Serialize, serde::Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct Publication {
    pub publisher: Option<Arc<dyn TPublisher>>,
    pub venue: Option<Arc<dyn TPublicationVenue>>,
    pub work: Arc<dyn TWork>,
    pub publication_date: NaiveDateTime,
    pub publication_notes: Vec<String>,
    pub version_number: Option<String>,
}

impl TPublication for Publication {
    fn version_number(&self) -> Option<&str> {
        self.version_number.as_deref()
    }
}
#[typetag::serde(name = "publication")]
impl TPublicationEvent for Publication {
    fn publisher(&self) -> Option<&Arc<dyn TPublisher>> {
        self.publisher.as_ref()
    }

    fn venue(&self) -> Option<&Arc<dyn TPublicationVenue>> {
        self.venue.as_ref()
    }

    fn work(&self) -> &Arc<dyn TWork> {
        &self.work
    }

    fn publication_date(&self) -> NaiveDateTime {
        self.publication_date
    }

    fn publication_notes(&self) -> &[String] {
        &self.publication_notes
    }

    fn version_number(&self) -> Option<&str> {
        self.version_number.as_deref()
    }

    fn relation_type(&self) -> &'static str {
        "publication"
    }
}
