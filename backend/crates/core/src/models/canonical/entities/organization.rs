use enum_dispatch::enum_dispatch;
use std::sync::Arc;

use crate::models::canonical::relations::{
    affiliation::EAffiliation, contribution::EContribution, publication_event::EPublicationEvent,
};

#[enum_dispatch]
pub trait TOrganization: Send + Sync {
    fn organization_id(&self) -> &str;
    fn organization_name(&self) -> &str;
    fn ror_id(&self) -> Option<&str>;
    fn entity_type(&self) -> &'static str;
}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct Organization {
    pub organization_id: String,
    pub organization_name: String,
    pub ror_id: Option<String>,
}

#[derive(bon::Builder)]
pub struct AOrganization {
    pub organization: Organization,
    pub contributions: Vec<Arc<EContribution>>,
    pub affiliations: Vec<Arc<EAffiliation>>,
    pub publication_events: Vec<Arc<EPublicationEvent>>,
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

    fn entity_type(&self) -> &'static str {
        "organization"
    }
}

pub trait TInstitution: TOrganization {}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct Institution {
    pub organization_id: String,
    pub organization_name: String,
    pub ror_id: Option<String>,
}

#[derive(bon::Builder)]
pub struct AInstitution {
    pub institution: Institution,
    pub contributions: Vec<Arc<EContribution>>,
    pub affiliations: Vec<Arc<EAffiliation>>,
    pub publication_events: Vec<Arc<EPublicationEvent>>,
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

    fn entity_type(&self) -> &'static str {
        "institution"
    }
}
impl TInstitution for Institution {}

pub trait TGovernmentInstitution: TInstitution {}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct GovernmentInstitution {
    pub organization_id: String,
    pub organization_name: String,
    pub ror_id: Option<String>,
}

#[derive(bon::Builder)]
pub struct AGovernmentInstitution {
    pub government_institution: GovernmentInstitution,
    pub contributions: Vec<Arc<EContribution>>,
    pub affiliations: Vec<Arc<EAffiliation>>,
    pub publication_events: Vec<Arc<EPublicationEvent>>,
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

    fn entity_type(&self) -> &'static str {
        "government_institution"
    }
}
impl TInstitution for GovernmentInstitution {}
impl TGovernmentInstitution for GovernmentInstitution {}

pub trait TEducationalInstitution: TInstitution {}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct EducationalInstitution {
    pub organization_id: String,
    pub organization_name: String,
    pub ror_id: Option<String>,
}

#[derive(bon::Builder)]
pub struct AEducationalInstitution {
    pub educational_institution: EducationalInstitution,
    pub contributions: Vec<Arc<EContribution>>,
    pub affiliations: Vec<Arc<EAffiliation>>,
    pub publication_events: Vec<Arc<EPublicationEvent>>,
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

    fn entity_type(&self) -> &'static str {
        "educational_institution"
    }
}
impl TInstitution for EducationalInstitution {}
impl TEducationalInstitution for EducationalInstitution {}

pub trait TNonprofitInstitution: TInstitution {}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct NonprofitInstitution {
    pub organization_id: String,
    pub organization_name: String,
    pub ror_id: Option<String>,
}

#[derive(bon::Builder)]
pub struct ANonprofitInstitution {
    pub nonprofit_institution: NonprofitInstitution,
    pub contributions: Vec<Arc<EContribution>>,
    pub affiliations: Vec<Arc<EAffiliation>>,
    pub publication_events: Vec<Arc<EPublicationEvent>>,
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

    fn entity_type(&self) -> &'static str {
        "nonprofit_institution"
    }
}
impl TInstitution for NonprofitInstitution {}
impl TNonprofitInstitution for NonprofitInstitution {}

pub trait TPublisher: TOrganization {}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct Publisher {
    pub organization_id: String,
    pub organization_name: String,
    pub ror_id: Option<String>,
}

#[derive(bon::Builder)]
pub struct APublisher {
    pub publisher: Publisher,
    pub contributions: Vec<Arc<EContribution>>,
    pub affiliations: Vec<Arc<EAffiliation>>,
    pub publication_events: Vec<Arc<EPublicationEvent>>,
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

    fn entity_type(&self) -> &'static str {
        "publisher"
    }
}
impl TPublisher for Publisher {}

#[enum_dispatch(TOrganization)]
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EOrganization {
    Organization,
    Institution,
    GovernmentInstitution,
    EducationalInstitution,
    NonprofitInstitution,
    Publisher,
}
