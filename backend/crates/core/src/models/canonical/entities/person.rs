use crate::models::canonical::relations::{affiliation, contribution};
use nonempty_collections::NEVec;
use std::sync::Arc;

#[typetag::serde(tag = "type")]
pub trait TPerson: Send + Sync {
    fn person_id(&self) -> &str;
    fn given_name(&self) -> Option<&str>;
    fn family_name(&self) -> Option<&str>;
}

#[typetag::serde(name = "person")]
impl affiliation::TPerson for Person {
    fn person_id(&self) -> &str {
        &self.person_id
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder)]
#[builder(on(String, into))]
pub struct Person {
    pub person_id: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
}

#[derive(bon::Builder)]
pub struct APerson {
    pub person: Person,
    pub contributions: NEVec<Arc<dyn contribution::TContribution>>,
    pub affiliations: Vec<Arc<dyn affiliation::TAffiliation>>,
}

#[typetag::serde(name = "person")]
impl contribution::TContributor for Person {
    fn contributor_id(&self) -> &str {
        &self.person_id
    }

    fn contributor_kind(&self) -> contribution::ContributorKind {
        contribution::ContributorKind::Person
    }
}
#[typetag::serde(name = "person")]
impl TPerson for Person {
    fn person_id(&self) -> &str {
        &self.person_id
    }

    fn given_name(&self) -> Option<&str> {
        self.given_name.as_deref()
    }

    fn family_name(&self) -> Option<&str> {
        self.family_name.as_deref()
    }
}
