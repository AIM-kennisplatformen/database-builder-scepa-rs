pub mod model;

pub use model::{
    CanonicalAffiliation, CanonicalContribution, CanonicalContributor, CanonicalDocument,
    CanonicalModel, CanonicalOrganization, CanonicalPublicationEvent,
    CanonicalPublicationEventKind, CanonicalPublicationVenue,
};

pub mod entities {
    pub mod document;
    pub mod organization;
    pub mod person;
    pub mod publication_venue;
}

pub mod relations {
    pub mod affiliation;
    pub mod contribution;
    pub mod publication_event;
}
