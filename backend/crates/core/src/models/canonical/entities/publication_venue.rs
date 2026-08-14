use crate::models::canonical::relations::publication_event;
use std::sync::Arc;

#[typetag::serde(tag = "type")]
pub trait TPublicationVenue: Send + Sync {
    fn venue_id(&self) -> &str;
    fn issn(&self) -> Option<&str>;
    fn venue_name(&self) -> &str;
    fn entity_type(&self) -> &'static str;
}

pub trait TJournal: TPublicationVenue {}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct Journal {
    pub venue_id: String,
    pub issn: Option<String>,
    pub venue_name: String,
}

#[derive(bon::Builder)]
pub struct AJournal {
    pub journal: Journal,
    pub publication_events: Vec<Arc<dyn publication_event::TPublicationEvent>>,
}

#[typetag::serde(name = "journal")]
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

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct Conference {
    pub venue_id: String,
    pub issn: Option<String>,
    pub venue_name: String,
}

#[derive(bon::Builder)]
pub struct AConference {
    pub conference: Conference,
    pub publication_events: Vec<Arc<dyn publication_event::TPublicationEvent>>,
}

#[typetag::serde(name = "conference")]
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

#[typetag::serde(name = "journal")]
impl publication_event::TPublicationVenue for Journal {
    fn venue_id(&self) -> &str {
        TPublicationVenue::venue_id(self)
    }
}

#[typetag::serde(name = "conference")]
impl publication_event::TPublicationVenue for Conference {
    fn venue_id(&self) -> &str {
        TPublicationVenue::venue_id(self)
    }
}
