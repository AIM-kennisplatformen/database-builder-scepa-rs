use crate::models::canonical::relations::{affiliation, contribution};
use nonempty_collections::NEVec;
use std::rc::Rc;

pub trait TPerson {
    fn person_id(&self) -> &str;
    fn given_name(&self) -> Option<&str>;
    fn family_name(&self) -> Option<&str>;
}

impl<T: TPerson + ?Sized> affiliation::TPerson for T {}

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
    pub contributions: NEVec<Rc<dyn contribution::TContribution>>,
    pub affiliations: Vec<Rc<dyn affiliation::TAffiliation>>,
}

impl contribution::TContributor for Person {}
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
