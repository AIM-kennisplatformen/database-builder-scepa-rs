//! The `affiliation` relation and its roles.

use std::sync::Arc;

use enum_dispatch::enum_dispatch;
use nonempty_collections::NEVec;

use crate::models::canonical::entities::{
    document::EDocument, organization::EOrganization, person::EPerson,
};

#[enum_dispatch]
pub trait TAffiliation: Send + Sync {
    fn person(&self) -> &EPerson;
    fn organization(&self) -> &EOrganization;
    fn evidence(&self) -> &[Arc<EDocument>];
}

#[derive(
    Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, bon::Builder, utoipa::ToSchema,
)]
pub struct Affiliation {
    pub person: Arc<EPerson>,
    pub organization: Arc<EOrganization>,
    #[schema(value_type = Vec<Arc<EDocument>>, min_items = 1)]
    pub evidence: NEVec<Arc<EDocument>>,
}

impl TAffiliation for Affiliation {
    fn person(&self) -> &EPerson {
        self.person.as_ref()
    }

    fn organization(&self) -> &EOrganization {
        self.organization.as_ref()
    }

    fn evidence(&self) -> &[Arc<EDocument>] {
        self.evidence.as_ref().as_slice()
    }
}

#[enum_dispatch(TAffiliation)]
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EAffiliation {
    Affiliation,
}
