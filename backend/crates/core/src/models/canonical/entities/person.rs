use enum_dispatch::enum_dispatch;
use nonempty_collections::NEVec;
use std::sync::Arc;

use crate::models::canonical::relations::{affiliation::EAffiliation, contribution::EContribution};

#[enum_dispatch]
pub trait TPerson: Send + Sync {
    fn person_id(&self) -> &str;
    fn given_name(&self) -> Option<&str>;
    fn family_name(&self) -> Option<&str>;
}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
#[builder(on(String, into))]
pub struct Person {
    pub person_id: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
}

#[derive(bon::Builder)]
pub struct APerson {
    pub person: Person,
    pub contributions: NEVec<Arc<EContribution>>,
    pub affiliations: Vec<Arc<EAffiliation>>,
}

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

#[enum_dispatch(TPerson)]
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EPerson {
    Person,
}
