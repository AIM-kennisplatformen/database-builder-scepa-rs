use enum_dispatch::enum_dispatch;
use std::sync::Arc;

use crate::models::canonical::relations::publication_event::EPublicationEvent;

#[enum_dispatch]
pub trait TPublicationVenue: Send + Sync {
    fn venue_id(&self) -> &str;
    fn issn(&self) -> Option<&str>;
    fn venue_name(&self) -> &str;
    fn entity_type(&self) -> &'static str;
}

pub trait TJournal: TPublicationVenue {}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct Journal {
    pub venue_id: String,
    pub issn: Option<String>,
    pub venue_name: String,
}

#[derive(bon::Builder)]
pub struct AJournal {
    pub journal: Journal,
    pub publication_events: Vec<Arc<EPublicationEvent>>,
}

impl TPublicationVenue for Journal {
    fn venue_id(&self) -> &str {
        &self.venue_id
    }

    fn issn(&self) -> Option<&str> {
        self.issn.as_deref()
    }

    fn venue_name(&self) -> &str {
        &self.venue_name
    }

    fn entity_type(&self) -> &'static str {
        "journal"
    }
}
impl TJournal for Journal {}

pub trait TConference: TPublicationVenue {}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct Conference {
    pub venue_id: String,
    pub issn: Option<String>,
    pub venue_name: String,
}

#[derive(bon::Builder)]
pub struct AConference {
    pub conference: Conference,
    pub publication_events: Vec<Arc<EPublicationEvent>>,
}

impl TPublicationVenue for Conference {
    fn venue_id(&self) -> &str {
        &self.venue_id
    }

    fn issn(&self) -> Option<&str> {
        self.issn.as_deref()
    }

    fn venue_name(&self) -> &str {
        &self.venue_name
    }

    fn entity_type(&self) -> &'static str {
        "conference"
    }
}
impl TConference for Conference {}

#[enum_dispatch(TPublicationVenue)]
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EPublicationVenue {
    Journal,
    Conference,
}
