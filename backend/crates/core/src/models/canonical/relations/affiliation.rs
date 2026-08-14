//! The `affiliation` relation and its roles.

use nonempty_collections::NEVec;
use std::sync::Arc;

#[typetag::serde(tag = "type")]
pub trait TAffiliation: Send + Sync {
    fn person(&self) -> &Arc<dyn TPerson>;
    fn organization(&self) -> &Arc<dyn TOrganization>;
    fn evidence(&self) -> &[Arc<dyn TEvidence>];
}

#[typetag::serde(tag = "type")]
pub trait TPerson: Send + Sync {
    fn person_id(&self) -> &str;
}
#[typetag::serde(tag = "type")]
pub trait TOrganization: Send + Sync {
    fn organization_id(&self) -> &str;
}
#[typetag::serde(tag = "type")]
pub trait TEvidence: Send + Sync {
    fn document_id(&self) -> &str;
}

#[derive(serde::Serialize, serde::Deserialize, bon::Builder)]
pub struct Affiliation {
    pub person: Arc<dyn TPerson>,
    pub organization: Arc<dyn TOrganization>,
    pub evidence: NEVec<Arc<dyn TEvidence>>,
}

#[typetag::serde(name = "affiliation")]
impl TAffiliation for Affiliation {
    fn person(&self) -> &Arc<dyn TPerson> {
        &self.person
    }

    fn organization(&self) -> &Arc<dyn TOrganization> {
        &self.organization
    }

    fn evidence(&self) -> &[Arc<dyn TEvidence>] {
        self.evidence.as_ref().as_slice()
    }
}
