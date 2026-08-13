use crate::models::canonical::relations::publication_event;
use std::rc::Rc;

pub trait TPublicationVenue {
    fn venue_id(&self) -> &str;
    fn issn(&self) -> Option<&str>;
    fn venue_name(&self) -> &str;
}

impl<T: TPublicationVenue + ?Sized> publication_event::TPublicationVenue for T {}

pub trait TJournal: TPublicationVenue {}

#[derive(bon::Builder)]
#[builder(on(String, into))]
pub struct Journal {
    pub venue_id: String,
    pub issn: Option<String>,
    pub venue_name: String,
}

#[derive(bon::Builder)]
pub struct AJournal {
    pub journal: Journal,
    pub publication_events: Vec<Rc<dyn publication_event::TPublicationEvent>>,
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
}
impl TJournal for Journal {}

pub trait TConference: TPublicationVenue {}

#[derive(bon::Builder)]
#[builder(on(String, into))]
pub struct Conference {
    pub venue_id: String,
    pub issn: Option<String>,
    pub venue_name: String,
}

#[derive(bon::Builder)]
pub struct AConference {
    pub conference: Conference,
    pub publication_events: Vec<Rc<dyn publication_event::TPublicationEvent>>,
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
}
impl TConference for Conference {}
