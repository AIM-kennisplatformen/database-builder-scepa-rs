use crate::models::canonical::relations::{affiliation, contribution, publication_event};
use std::rc::Rc;

pub trait TOrganization {
    fn organization_id(&self) -> &str;
    fn organization_name(&self) -> &str;
    fn ror_id(&self) -> Option<&str>;
}

impl<T: TOrganization + ?Sized> affiliation::TOrganization for T {}
impl<T: TOrganization + ?Sized> publication_event::TPublisher for T {}
impl<T: TOrganization + ?Sized> contribution::TContributor for T {}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct Organization {
    pub organization_id: String,
    pub organization_name: String,
    pub ror_id: Option<String>,
}

#[derive(bon::Builder)]
pub struct AOrganization {
    pub organization: Organization,
    pub contributions: Vec<Rc<dyn contribution::TContribution>>,
    pub affiliations: Vec<Rc<dyn affiliation::TAffiliation>>,
    pub publication_events: Vec<Rc<dyn publication_event::TPublicationEvent>>,
}

impl TOrganization for Organization {
    fn organization_id(&self) -> &str {
        &self.organization_id
    }

    fn organization_name(&self) -> &str {
        &self.organization_name
    }

    fn ror_id(&self) -> Option<&str> {
        self.ror_id.as_deref()
    }
}

pub trait TInstitution: TOrganization {}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct Institution {
    pub organization_id: String,
    pub organization_name: String,
    pub ror_id: Option<String>,
}

#[derive(bon::Builder)]
pub struct AInstitution {
    pub institution: Institution,
    pub contributions: Vec<Rc<dyn contribution::TContribution>>,
    pub affiliations: Vec<Rc<dyn affiliation::TAffiliation>>,
    pub publication_events: Vec<Rc<dyn publication_event::TPublicationEvent>>,
}

impl TOrganization for Institution {
    fn organization_id(&self) -> &str {
        &self.organization_id
    }

    fn organization_name(&self) -> &str {
        &self.organization_name
    }

    fn ror_id(&self) -> Option<&str> {
        self.ror_id.as_deref()
    }
}
impl TInstitution for Institution {}

pub trait TGovernmentInstitution: TInstitution {}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct GovernmentInstitution {
    pub organization_id: String,
    pub organization_name: String,
    pub ror_id: Option<String>,
}

#[derive(bon::Builder)]
pub struct AGovernmentInstitution {
    pub government_institution: GovernmentInstitution,
    pub contributions: Vec<Rc<dyn contribution::TContribution>>,
    pub affiliations: Vec<Rc<dyn affiliation::TAffiliation>>,
    pub publication_events: Vec<Rc<dyn publication_event::TPublicationEvent>>,
}

impl TOrganization for GovernmentInstitution {
    fn organization_id(&self) -> &str {
        &self.organization_id
    }

    fn organization_name(&self) -> &str {
        &self.organization_name
    }

    fn ror_id(&self) -> Option<&str> {
        self.ror_id.as_deref()
    }
}
impl TInstitution for GovernmentInstitution {}
impl TGovernmentInstitution for GovernmentInstitution {}

pub trait TEducationalInstitution: TInstitution {}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct EducationalInstitution {
    pub organization_id: String,
    pub organization_name: String,
    pub ror_id: Option<String>,
}

#[derive(bon::Builder)]
pub struct AEducationalInstitution {
    pub educational_institution: EducationalInstitution,
    pub contributions: Vec<Rc<dyn contribution::TContribution>>,
    pub affiliations: Vec<Rc<dyn affiliation::TAffiliation>>,
    pub publication_events: Vec<Rc<dyn publication_event::TPublicationEvent>>,
}

impl TOrganization for EducationalInstitution {
    fn organization_id(&self) -> &str {
        &self.organization_id
    }

    fn organization_name(&self) -> &str {
        &self.organization_name
    }

    fn ror_id(&self) -> Option<&str> {
        self.ror_id.as_deref()
    }
}
impl TInstitution for EducationalInstitution {}
impl TEducationalInstitution for EducationalInstitution {}

pub trait TNonprofitInstitution: TInstitution {}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct NonprofitInstitution {
    pub organization_id: String,
    pub organization_name: String,
    pub ror_id: Option<String>,
}

#[derive(bon::Builder)]
pub struct ANonprofitInstitution {
    pub nonprofit_institution: NonprofitInstitution,
    pub contributions: Vec<Rc<dyn contribution::TContribution>>,
    pub affiliations: Vec<Rc<dyn affiliation::TAffiliation>>,
    pub publication_events: Vec<Rc<dyn publication_event::TPublicationEvent>>,
}

impl TOrganization for NonprofitInstitution {
    fn organization_id(&self) -> &str {
        &self.organization_id
    }

    fn organization_name(&self) -> &str {
        &self.organization_name
    }

    fn ror_id(&self) -> Option<&str> {
        self.ror_id.as_deref()
    }
}
impl TInstitution for NonprofitInstitution {}
impl TNonprofitInstitution for NonprofitInstitution {}

pub trait TPublisher: TOrganization {}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct Publisher {
    pub organization_id: String,
    pub organization_name: String,
    pub ror_id: Option<String>,
}

#[derive(bon::Builder)]
pub struct APublisher {
    pub publisher: Publisher,
    pub contributions: Vec<Rc<dyn contribution::TContribution>>,
    pub affiliations: Vec<Rc<dyn affiliation::TAffiliation>>,
    pub publication_events: Vec<Rc<dyn publication_event::TPublicationEvent>>,
}

impl TOrganization for Publisher {
    fn organization_id(&self) -> &str {
        &self.organization_id
    }

    fn organization_name(&self) -> &str {
        &self.organization_name
    }

    fn ror_id(&self) -> Option<&str> {
        self.ror_id.as_deref()
    }
}
impl TPublisher for Publisher {}
