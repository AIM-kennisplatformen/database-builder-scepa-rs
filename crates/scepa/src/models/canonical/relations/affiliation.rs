//! The `affiliation` relation and its roles.

use nonempty_collections::NEVec;
use std::rc::Rc;

pub trait TAffiliation {
    fn person(&self) -> &Rc<dyn TPerson>;
    fn organization(&self) -> &Rc<dyn TOrganization>;
    fn evidence(&self) -> &[Rc<dyn TEvidence>];
}

pub trait TPerson {}
pub trait TOrganization {}
pub trait TEvidence {}

#[derive(bon::Builder)]
pub struct Affiliation {
    pub person: Rc<dyn TPerson>,
    pub organization: Rc<dyn TOrganization>,
    pub evidence: NEVec<Rc<dyn TEvidence>>,
}

impl TAffiliation for Affiliation {
    fn person(&self) -> &Rc<dyn TPerson> {
        &self.person
    }

    fn organization(&self) -> &Rc<dyn TOrganization> {
        &self.organization
    }

    fn evidence(&self) -> &[Rc<dyn TEvidence>] {
        self.evidence.as_ref().as_slice()
    }
}
