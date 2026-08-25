//! The abstract `contribution` relation and its descendants.

use std::sync::Arc;

use enum_dispatch::enum_dispatch;

use crate::models::canonical::entities::{
    document::EDocument,
    organization::{EOrganization, TOrganization},
    person::{EPerson, TPerson},
};

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ContributorKind {
    Person,
    Organization,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(untagged)]
pub enum EContributor {
    Person(Arc<EPerson>),
    Organization(Arc<EOrganization>),
}

impl EContributor {
    pub fn contributor_id(&self) -> &str {
        match self {
            Self::Person(person) => person.person_id(),
            Self::Organization(organization) => organization.organization_id(),
        }
    }

    pub const fn contributor_kind(&self) -> ContributorKind {
        match self {
            Self::Person(_) => ContributorKind::Person,
            Self::Organization(_) => ContributorKind::Organization,
        }
    }
}

#[enum_dispatch]
pub trait TContribution: Send + Sync {
    fn contributor(&self) -> &EContributor;
    fn work(&self) -> &EDocument;
    fn relation_type(&self) -> &'static str;
}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
pub struct Contribution {
    pub contributor: Arc<EContributor>,
    pub work: Arc<EDocument>,
}

impl TContribution for Contribution {
    fn contributor(&self) -> &EContributor {
        self.contributor.as_ref()
    }

    fn work(&self) -> &EDocument {
        self.work.as_ref()
    }

    fn relation_type(&self) -> &'static str {
        "contribution"
    }
}

pub trait TAuthorship: TContribution {}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
pub struct Authorship {
    pub contributor: Arc<EContributor>,
    pub work: Arc<EDocument>,
}

impl TAuthorship for Authorship {}
impl TContribution for Authorship {
    fn contributor(&self) -> &EContributor {
        self.contributor.as_ref()
    }

    fn work(&self) -> &EDocument {
        self.work.as_ref()
    }

    fn relation_type(&self) -> &'static str {
        "authorship"
    }
}

pub trait TPeerReview: TContribution {}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
pub struct PeerReview {
    pub contributor: Arc<EContributor>,
    pub work: Arc<EDocument>,
}

impl TPeerReview for PeerReview {}
impl TContribution for PeerReview {
    fn contributor(&self) -> &EContributor {
        self.contributor.as_ref()
    }

    fn work(&self) -> &EDocument {
        self.work.as_ref()
    }

    fn relation_type(&self) -> &'static str {
        "peer_review"
    }
}

#[enum_dispatch(TContribution)]
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EContribution {
    Contribution,
    Authorship,
    PeerReview,
}
