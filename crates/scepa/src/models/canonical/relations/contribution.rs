//! The abstract `contribution` relation and its descendants.

use std::rc::Rc;

pub trait TContributor {}
pub trait TWork {}

pub trait TContribution {
    fn contributor(&self) -> &Rc<dyn TContributor>;
    fn work(&self) -> &Rc<dyn TWork>;
}

#[derive(bon::Builder)]
pub struct Contribution {
    pub contributor: Rc<dyn TContributor>,
    pub work: Rc<dyn TWork>,
}

impl TContribution for Contribution {
    fn contributor(&self) -> &Rc<dyn TContributor> {
        &self.contributor
    }

    fn work(&self) -> &Rc<dyn TWork> {
        &self.work
    }
}

pub trait TAuthorship: TContribution {}

#[derive(bon::Builder)]
pub struct Authorship {
    pub contributor: Rc<dyn TContributor>,
    pub work: Rc<dyn TWork>,
}

impl TAuthorship for Authorship {}
impl TContribution for Authorship {
    fn contributor(&self) -> &Rc<dyn TContributor> {
        &self.contributor
    }

    fn work(&self) -> &Rc<dyn TWork> {
        &self.work
    }
}

pub trait TPeerReview: TContribution {}

#[derive(bon::Builder)]
pub struct PeerReview {
    pub contributor: Rc<dyn TContributor>,
    pub work: Rc<dyn TWork>,
}

impl TPeerReview for PeerReview {}
impl TContribution for PeerReview {
    fn contributor(&self) -> &Rc<dyn TContributor> {
        &self.contributor
    }

    fn work(&self) -> &Rc<dyn TWork> {
        &self.work
    }
}
