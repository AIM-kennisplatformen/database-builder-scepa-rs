//! The abstract `contribution` relation and its descendants.

use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributorKind {
    Person,
    Organization,
}

#[typetag::serde(tag = "type")]
pub trait TContributor: Send + Sync {
    fn contributor_id(&self) -> &str;
    fn contributor_kind(&self) -> ContributorKind;
}

#[typetag::serde(tag = "type")]
pub trait TWork: Send + Sync {
    fn document_id(&self) -> &str;
}

#[typetag::serde(tag = "type")]
pub trait TContribution: Send + Sync {
    fn contributor(&self) -> &Arc<dyn TContributor>;
    fn work(&self) -> &Arc<dyn TWork>;
    fn relation_type(&self) -> &'static str;
}

#[derive(serde::Serialize, serde::Deserialize, bon::Builder)]
pub struct Contribution {
    pub contributor: Arc<dyn TContributor>,
    pub work: Arc<dyn TWork>,
}

#[typetag::serde(name = "contribution")]
impl TContribution for Contribution {
    fn contributor(&self) -> &Arc<dyn TContributor> {
        &self.contributor
    }

    fn work(&self) -> &Arc<dyn TWork> {
        &self.work
    }

    fn relation_type(&self) -> &'static str {
        "contribution"
    }
}

pub trait TAuthorship: TContribution {}

#[derive(serde::Serialize, serde::Deserialize, bon::Builder)]
pub struct Authorship {
    pub contributor: Arc<dyn TContributor>,
    pub work: Arc<dyn TWork>,
}

impl TAuthorship for Authorship {}
#[typetag::serde(name = "authorship")]
impl TContribution for Authorship {
    fn contributor(&self) -> &Arc<dyn TContributor> {
        &self.contributor
    }

    fn work(&self) -> &Arc<dyn TWork> {
        &self.work
    }

    fn relation_type(&self) -> &'static str {
        "authorship"
    }
}

pub trait TPeerReview: TContribution {}

#[derive(serde::Serialize, serde::Deserialize, bon::Builder)]
pub struct PeerReview {
    pub contributor: Arc<dyn TContributor>,
    pub work: Arc<dyn TWork>,
}

impl TPeerReview for PeerReview {}
#[typetag::serde(name = "peer_review")]
impl TContribution for PeerReview {
    fn contributor(&self) -> &Arc<dyn TContributor> {
        &self.contributor
    }

    fn work(&self) -> &Arc<dyn TWork> {
        &self.work
    }

    fn relation_type(&self) -> &'static str {
        "peer_review"
    }
}
