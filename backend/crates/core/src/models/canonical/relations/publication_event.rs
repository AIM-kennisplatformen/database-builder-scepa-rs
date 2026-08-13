//! The abstract `publication_event` relation and its descendants.

use chrono::NaiveDateTime;
use std::rc::Rc;

pub trait TPublisher {}
pub trait TPublicationVenue {}
pub trait TWork {}

/// Common roles and attributes of every publication event.
pub trait TPublicationEvent {
    fn publisher(&self) -> Option<&Rc<dyn TPublisher>>;
    fn venue(&self) -> Option<&Rc<dyn TPublicationVenue>>;
    fn work(&self) -> &Rc<dyn TWork>;
    fn publication_date(&self) -> NaiveDateTime;
    fn publication_notes(&self) -> &[String];
}

pub trait TSubmission: TPublicationEvent {}

#[derive(bon::Builder)]
#[builder(on(String, into))]
pub struct Submission {
    pub publisher: Option<Rc<dyn TPublisher>>,
    pub venue: Option<Rc<dyn TPublicationVenue>>,
    pub work: Rc<dyn TWork>,
    pub publication_date: NaiveDateTime,
    pub publication_notes: Vec<String>,
}

impl TSubmission for Submission {}
impl TPublicationEvent for Submission {
    fn publisher(&self) -> Option<&Rc<dyn TPublisher>> {
        self.publisher.as_ref()
    }

    fn venue(&self) -> Option<&Rc<dyn TPublicationVenue>> {
        self.venue.as_ref()
    }

    fn work(&self) -> &Rc<dyn TWork> {
        &self.work
    }

    fn publication_date(&self) -> NaiveDateTime {
        self.publication_date
    }

    fn publication_notes(&self) -> &[String] {
        &self.publication_notes
    }
}

pub trait TAcceptance: TPublicationEvent {}

#[derive(bon::Builder)]
#[builder(on(String, into))]
pub struct Acceptance {
    pub publisher: Option<Rc<dyn TPublisher>>,
    pub venue: Option<Rc<dyn TPublicationVenue>>,
    pub work: Rc<dyn TWork>,
    pub publication_date: NaiveDateTime,
    pub publication_notes: Vec<String>,
}

impl TAcceptance for Acceptance {}
impl TPublicationEvent for Acceptance {
    fn publisher(&self) -> Option<&Rc<dyn TPublisher>> {
        self.publisher.as_ref()
    }

    fn venue(&self) -> Option<&Rc<dyn TPublicationVenue>> {
        self.venue.as_ref()
    }

    fn work(&self) -> &Rc<dyn TWork> {
        &self.work
    }

    fn publication_date(&self) -> NaiveDateTime {
        self.publication_date
    }

    fn publication_notes(&self) -> &[String] {
        &self.publication_notes
    }
}

pub trait TPublication: TPublicationEvent {
    fn version_number(&self) -> Option<&str>;
}

#[derive(bon::Builder)]
#[builder(on(String, into))]
pub struct Publication {
    pub publisher: Option<Rc<dyn TPublisher>>,
    pub venue: Option<Rc<dyn TPublicationVenue>>,
    pub work: Rc<dyn TWork>,
    pub publication_date: NaiveDateTime,
    pub publication_notes: Vec<String>,
    pub version_number: Option<String>,
}

impl TPublication for Publication {
    fn version_number(&self) -> Option<&str> {
        self.version_number.as_deref()
    }
}
impl TPublicationEvent for Publication {
    fn publisher(&self) -> Option<&Rc<dyn TPublisher>> {
        self.publisher.as_ref()
    }

    fn venue(&self) -> Option<&Rc<dyn TPublicationVenue>> {
        self.venue.as_ref()
    }

    fn work(&self) -> &Rc<dyn TWork> {
        &self.work
    }

    fn publication_date(&self) -> NaiveDateTime {
        self.publication_date
    }

    fn publication_notes(&self) -> &[String] {
        &self.publication_notes
    }
}
