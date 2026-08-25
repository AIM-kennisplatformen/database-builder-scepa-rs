//! The abstract `publication_event` relation and its descendants.

use std::sync::Arc;

use chrono::NaiveDateTime;
use enum_dispatch::enum_dispatch;

use crate::models::canonical::entities::{
    document::EDocument, organization::EOrganization, publication_venue::EPublicationVenue,
};

#[enum_dispatch]
pub trait TPublicationEvent: Send + Sync {
    fn publisher(&self) -> Option<&EOrganization>;
    fn venue(&self) -> Option<&EPublicationVenue>;
    fn work(&self) -> &EDocument;
    fn publication_date(&self) -> NaiveDateTime;
    fn publication_notes(&self) -> &[String];
    fn version_number(&self) -> Option<&str> {
        None
    }
    fn relation_type(&self) -> &'static str;
}

pub trait TSubmission: TPublicationEvent {}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct Submission {
    pub publisher: Option<Arc<EOrganization>>,
    pub venue: Option<Arc<EPublicationVenue>>,
    pub work: Arc<EDocument>,
    #[schema(value_type = String, format = DateTime)]
    pub publication_date: NaiveDateTime,
    pub publication_notes: Vec<String>,
}

impl TSubmission for Submission {}
impl TPublicationEvent for Submission {
    fn publisher(&self) -> Option<&EOrganization> {
        self.publisher.as_deref()
    }

    fn venue(&self) -> Option<&EPublicationVenue> {
        self.venue.as_deref()
    }

    fn work(&self) -> &EDocument {
        self.work.as_ref()
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

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct Acceptance {
    pub publisher: Option<Arc<EOrganization>>,
    pub venue: Option<Arc<EPublicationVenue>>,
    pub work: Arc<EDocument>,
    #[schema(value_type = String, format = DateTime)]
    pub publication_date: NaiveDateTime,
    pub publication_notes: Vec<String>,
}

impl TAcceptance for Acceptance {}
impl TPublicationEvent for Acceptance {
    fn publisher(&self) -> Option<&EOrganization> {
        self.publisher.as_deref()
    }

    fn venue(&self) -> Option<&EPublicationVenue> {
        self.venue.as_deref()
    }

    fn work(&self) -> &EDocument {
        self.work.as_ref()
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

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct Publication {
    pub publisher: Option<Arc<EOrganization>>,
    pub venue: Option<Arc<EPublicationVenue>>,
    pub work: Arc<EDocument>,
    #[schema(value_type = String, format = DateTime)]
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
    fn publisher(&self) -> Option<&EOrganization> {
        self.publisher.as_deref()
    }

    fn venue(&self) -> Option<&EPublicationVenue> {
        self.venue.as_deref()
    }

    fn work(&self) -> &EDocument {
        self.work.as_ref()
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

#[enum_dispatch(TPublicationEvent)]
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EPublicationEvent {
    Submission,
    Acceptance,
    Publication,
}
