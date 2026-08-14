//! Service for validating draft documents and persisting canonical documents.

use std::sync::Arc;

use async_trait::async_trait;
use typedb_driver::{
    Addresses, Credentials, DriverOptions, DriverTlsConfig, TransactionType, TypeDBDriver,
    given::GivenRows,
};

use crate::models::{
    canonical::{CanonicalModel, entities, relations},
    draft::TeiDocument,
};
use entities::{
    document::TDocument, organization::TOrganization, person::TPerson,
    publication_venue::TPublicationVenue,
};
use relations::{
    affiliation::TAffiliation,
    contribution::{ContributorKind, TContribution},
    publication_event::TPublicationEvent,
};

/// Persistence boundary used by [`TypeDbService`].
///
/// Keeping this boundary smaller than the TypeDB driver makes the validation
/// and execution contract independently testable.
#[async_trait]
pub trait CanonicalDocumentStore: Send + Sync {
    async fn insert(&self, model: &CanonicalModel) -> eros::Result<()>;
    async fn update(
        &self,
        old: &CanonicalModel,
        new: &CanonicalModel,
    ) -> eros::Result<CanonicalUpdateSummary>;
}

/// The graph sections touched by one update transaction.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalUpdateSummary {
    pub document_changed: bool,
    pub contributors_deleted: usize,
    pub contributors_inserted: usize,
    pub organizations_deleted: usize,
    pub organizations_inserted: usize,
    pub venues_deleted: usize,
    pub venues_inserted: usize,
    pub affiliations_deleted: usize,
    pub affiliations_inserted: usize,
    pub publication_events_deleted: usize,
    pub publication_events_inserted: usize,
}

impl CanonicalUpdateSummary {
    pub fn changed(&self) -> bool {
        self.document_changed
            || self.contributors_deleted != 0
            || self.contributors_inserted != 0
            || self.organizations_deleted != 0
            || self.organizations_inserted != 0
            || self.venues_deleted != 0
            || self.venues_inserted != 0
            || self.affiliations_deleted != 0
            || self.affiliations_inserted != 0
            || self.publication_events_deleted != 0
            || self.publication_events_inserted != 0
    }
}

/// A TypeDB-backed canonical document store.
#[derive(Clone)]
pub struct TypeDbStore {
    driver: Arc<TypeDBDriver>,
    database: String,
}

impl TypeDbStore {
    pub fn new(driver: TypeDBDriver, database: impl Into<String>) -> Self {
        Self {
            driver: Arc::new(driver),
            database: database.into(),
        }
    }

    /// Creates the database when necessary and installs or verifies the
    /// application's TypeQL schema.
    pub async fn ensure_schema(&self) -> eros::Result<()> {
        if !self.driver.databases().contains(&self.database).await? {
            self.driver.databases().create(&self.database).await?;
            return self.install_schema().await;
        }

        if self.verify_schema().await.is_err() {
            self.install_schema().await?;
        }
        self.verify_schema().await
    }

    async fn install_schema(&self) -> eros::Result<()> {
        let transaction = self
            .driver
            .transaction(&self.database, TransactionType::Schema)
            .await?;
        transaction.query(include_str!("../../schema.tql")).await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn verify_schema(&self) -> eros::Result<()> {
        // Query analysis resolves labels against the active schema. This makes
        // startup fail fast if an existing database is empty or incompatible.
        let transaction = self
            .driver
            .transaction(&self.database, TransactionType::Read)
            .await?;
        transaction
            .analyze(
                "match \
                 $document label document; \
                 $research_paper label research_paper; \
                 $book label book; \
                 $person label person; \
                 $organization label organization; \
                 $publisher label publisher; \
                 $journal label journal; \
                 $authorship label authorship; \
                 $contribution label contribution; \
                 $affiliation label affiliation; \
                 $publication label publication; \
                 $document_id label document_id; \
                 $pdf_hash label pdf_hash; \
                 $person_id label person_id; \
                 $organization_id label organization_id; \
                 $venue_id label venue_id; \
                 $title label title;",
            )
            .await?;
        transaction.close().await?;
        Ok(())
    }
}

#[async_trait]
impl CanonicalDocumentStore for TypeDbStore {
    async fn insert(&self, model: &CanonicalModel) -> eros::Result<()> {
        let transaction = self
            .driver
            .transaction(&self.database, TransactionType::Write)
            .await?;

        let (query, rows) = document_insert_query(&model.document)?;
        transaction.query_with_rows(query, rows).await?;
        for person in &model.persons {
            let (query, rows) = person_insert_query(person.as_ref())?;
            transaction.query_with_rows(query, rows).await?;
        }
        for organization in &model.organizations {
            let (query, rows) = organization_insert_query(organization.as_ref())?;
            transaction.query_with_rows(query, rows).await?;
        }
        for venue in &model.publication_venues {
            let (query, rows) = publication_venue_insert_query(venue.as_ref())?;
            transaction.query_with_rows(query, rows).await?;
        }
        for contribution in &model.contributions {
            let (query, rows) = contribution_insert_query(contribution.as_ref())?;
            transaction.query_with_rows(query, rows).await?;
        }
        for affiliation in &model.affiliations {
            let (query, rows) = affiliation_insert_query(affiliation.as_ref())?;
            transaction.query_with_rows(query, rows).await?;
        }
        for event in &model.publication_events {
            let (query, rows) = publication_event_insert_query(event.as_ref())?;
            transaction.query_with_rows(query, rows).await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    async fn update(
        &self,
        old: &CanonicalModel,
        new: &CanonicalModel,
    ) -> eros::Result<CanonicalUpdateSummary> {
        let document_changed = document_value(&old.document) != document_value(&new.document);
        let summary = CanonicalUpdateSummary {
            document_changed,
            contributors_deleted: old.persons.len(),
            contributors_inserted: new.persons.len(),
            organizations_deleted: old.organizations.len(),
            organizations_inserted: new.organizations.len(),
            venues_deleted: old.publication_venues.len(),
            venues_inserted: new.publication_venues.len(),
            affiliations_deleted: old.affiliations.len(),
            affiliations_inserted: new.affiliations.len(),
            publication_events_deleted: old.publication_events.len(),
            publication_events_inserted: new.publication_events.len(),
        };
        if old == new {
            return Ok(CanonicalUpdateSummary::default());
        }

        let transaction = self
            .driver
            .transaction(&self.database, TransactionType::Write)
            .await?;

        let (query, rows) =
            document_relation_delete_query(&old.document, "affiliation", "evidence")?;
        transaction.query_with_rows(query, rows).await?;
        let (query, rows) =
            document_relation_delete_query(&old.document, "publication_event", "work")?;
        transaction.query_with_rows(query, rows).await?;
        let (query, rows) = document_relation_delete_query(&old.document, "contribution", "work")?;
        transaction.query_with_rows(query, rows).await?;
        for person in &old.persons {
            let (query, rows) = person_delete_query(person.as_ref())?;
            transaction.query_with_rows(query, rows).await?;
        }
        for organization in &old.organizations {
            let (query, rows) = organization_delete_query(organization)?;
            transaction.query_with_rows(query, rows).await?;
        }
        for venue in &old.publication_venues {
            let (query, rows) = publication_venue_delete_query(venue)?;
            transaction.query_with_rows(query, rows).await?;
        }
        let (query, rows) = document_delete_query(&old.document)?;
        transaction.query_with_rows(query, rows).await?;

        let (query, rows) = document_insert_query(&new.document)?;
        transaction.query_with_rows(query, rows).await?;
        for person in &new.persons {
            let (query, rows) = person_insert_query(person.as_ref())?;
            transaction.query_with_rows(query, rows).await?;
        }
        for organization in &new.organizations {
            let (query, rows) = organization_insert_query(organization.as_ref())?;
            transaction.query_with_rows(query, rows).await?;
        }
        for venue in &new.publication_venues {
            let (query, rows) = publication_venue_insert_query(venue.as_ref())?;
            transaction.query_with_rows(query, rows).await?;
        }
        for contribution in &new.contributions {
            let (query, rows) = contribution_insert_query(contribution.as_ref())?;
            transaction.query_with_rows(query, rows).await?;
        }
        for affiliation in &new.affiliations {
            let (query, rows) = affiliation_insert_query(affiliation.as_ref())?;
            transaction.query_with_rows(query, rows).await?;
        }
        for event in &new.publication_events {
            let (query, rows) = publication_event_insert_query(event.as_ref())?;
            transaction.query_with_rows(query, rows).await?;
        }

        transaction.commit().await?;
        Ok(summary)
    }
}

/// Validates a draft by canonicalising it, then writes only canonical models.
#[derive(Clone)]
pub struct TypeDbService<S> {
    store: S,
}

impl<S> TypeDbService<S> {
    pub const NAME: &'static str = "typedb";

    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Attempts the draft-to-canonical transformation.
    ///
    /// A successful result is the exact value accepted by [`Self::execute`].
    /// Transformation errors are validation failures and prevent execution.
    pub async fn pre_validate(&self, draft: &TeiDocument) -> eros::Result<CanonicalModel> {
        CanonicalModel::try_from(draft)
    }

    /// Canonicalises a document and attaches its content-addressed SHA-256.
    pub async fn pre_validate_with_pdf_hash(
        &self,
        draft: &TeiDocument,
        pdf_hash: &str,
    ) -> eros::Result<CanonicalModel> {
        CanonicalModel::try_from_with_pdf_hash(draft, pdf_hash)
    }
}

impl TypeDbService<TypeDbStore> {
    /// Connects to TypeDB without TLS and ensures the configured schema is
    /// active before returning the service.
    pub async fn connect(
        address: &str,
        database: impl Into<String>,
        username: &str,
        password: &str,
    ) -> eros::Result<Self> {
        let addresses = Addresses::try_from_address_str(address)?;
        let driver = TypeDBDriver::new(
            addresses,
            Credentials::new(username, password),
            DriverOptions::new(DriverTlsConfig::disabled()),
        )
        .await?;
        let service = Self::from_driver(driver, database);
        service.ensure_schema().await?;
        Ok(service)
    }

    /// Creates the service from a connected TypeDB driver and database name.
    pub fn from_driver(driver: TypeDBDriver, database: impl Into<String>) -> Self {
        Self::new(TypeDbStore::new(driver, database))
    }

    /// Ensures this service's database exists with the expected schema.
    pub async fn ensure_schema(&self) -> eros::Result<()> {
        self.store.ensure_schema().await
    }
}

impl<S> TypeDbService<S>
where
    S: CanonicalDocumentStore,
{
    /// Inserts a previously validated canonical model into TypeDB.
    pub async fn execute(&self, canonical: &CanonicalModel) -> eros::Result<()> {
        self.store.insert(canonical).await
    }

    /// Replaces only graph sections whose canonical values changed.
    pub async fn execute_update(
        &self,
        old: &CanonicalModel,
        new: &CanonicalModel,
    ) -> eros::Result<CanonicalUpdateSummary> {
        self.store.update(old, new).await
    }

    /// Runs pre-validation and inserts its canonical result.
    pub async fn run(&self, draft: &TeiDocument) -> eros::Result<CanonicalModel> {
        let canonical = self.pre_validate(draft).await?;
        self.execute(&canonical).await?;
        Ok(canonical)
    }
}

/// Alternate spelling retained for callers that spell the product as one word.
pub type TypedbService<S> = TypeDbService<S>;
/// Product-capitalised spelling of [`TypeDbService`].
pub type TypeDBService<S> = TypeDbService<S>;

fn document_insert_query(document: &Arc<dyn TDocument>) -> eros::Result<(String, GivenRows)> {
    let mut variables = vec!["document_id".to_owned(), "title".to_owned()];
    let mut declarations = vec!["$document_id: string", "$title: string"];
    let mut attributes = vec![
        "has document_id == $document_id".to_owned(),
        "has title == $title".to_owned(),
    ];
    let mut values = vec![
        document.document_id().to_owned().into(),
        document.title().to_owned().into(),
    ];

    if let Some(pdf_hash) = document.pdf_hash() {
        variables.push("pdf_hash".to_owned());
        declarations.push("$pdf_hash: string");
        attributes.push("has pdf_hash == $pdf_hash".to_owned());
        values.push(pdf_hash.to_owned().into());
    }

    if let Some(doi) = document.doi() {
        variables.push("doi".to_owned());
        declarations.push("$doi: string");
        attributes.push("has doi == $doi".to_owned());
        values.push(doi.to_owned().into());
    }
    if let Some(isbn) = document.isbn() {
        variables.push("isbn".to_owned());
        declarations.push("$isbn: string");
        attributes.push("has isbn == $isbn".to_owned());
        values.push(isbn.to_owned().into());
    }

    let query = format!(
        "given {}; insert $document isa {}, {};",
        declarations.join(", "),
        document.entity_type(),
        attributes.join(", ")
    );
    let mut rows = GivenRows::new(variables, 1);
    rows.push_row(values)?;
    Ok((query, rows))
}

fn person_insert_query(person: &dyn TPerson) -> eros::Result<(String, GivenRows)> {
    let mut variables = vec!["person_id".to_owned()];
    let mut declarations = vec!["$person_id: string"];
    let mut person_attributes = vec!["has person_id == $person_id".to_owned()];
    let mut values = vec![person.person_id().to_owned().into()];

    if let Some(given_name) = person.given_name() {
        variables.push("given_name".to_owned());
        declarations.push("$given_name: string");
        person_attributes.push("has given_name == $given_name".to_owned());
        values.push(given_name.to_owned().into());
    }
    if let Some(family_name) = person.family_name() {
        variables.push("family_name".to_owned());
        declarations.push("$family_name: string");
        person_attributes.push("has family_name == $family_name".to_owned());
        values.push(family_name.to_owned().into());
    }
    let query = format!(
        "given {}; insert $person isa person, {};",
        declarations.join(", "),
        person_attributes.join(", "),
    );
    let mut rows = GivenRows::new(variables, 1);
    rows.push_row(values)?;
    Ok((query, rows))
}

fn contribution_insert_query(
    contribution: &dyn TContribution,
) -> eros::Result<(String, GivenRows)> {
    let contributor = contribution.contributor();
    let contributor_type = match contributor.contributor_kind() {
        ContributorKind::Person => "person",
        ContributorKind::Organization => "organization",
    };
    let contributor_id_type = match contributor.contributor_kind() {
        ContributorKind::Person => "person_id",
        ContributorKind::Organization => "organization_id",
    };
    let query = format!(
        "given $document_id: string, $contributor_id: string; \
         match $document isa document, has document_id == $document_id; \
         $contributor isa {contributor_type}, has {contributor_id_type} == $contributor_id; \
         insert $contribution isa {}, links (contributor: $contributor, work: $document);",
        contribution.relation_type(),
    );
    let mut rows = GivenRows::new(
        vec!["document_id".to_owned(), "contributor_id".to_owned()],
        1,
    );
    rows.push_row(vec![
        contribution.work().document_id().to_owned().into(),
        contributor.contributor_id().to_owned().into(),
    ])?;
    Ok((query, rows))
}

fn organization_insert_query(
    organization: &dyn TOrganization,
) -> eros::Result<(String, GivenRows)> {
    let mut variables = vec!["organization_id".to_owned(), "organization_name".to_owned()];
    let mut declarations = vec!["$organization_id: string", "$organization_name: string"];
    let mut attributes = vec![
        "has organization_id == $organization_id".to_owned(),
        "has organization_name == $organization_name".to_owned(),
    ];
    let mut values = vec![
        organization.organization_id().to_owned().into(),
        organization.organization_name().to_owned().into(),
    ];
    if let Some(ror_id) = organization.ror_id() {
        variables.push("ror_id".to_owned());
        declarations.push("$ror_id: string");
        attributes.push("has ror_id == $ror_id".to_owned());
        values.push(ror_id.to_owned().into());
    }
    let query = format!(
        "given {}; insert $organization isa {}, {};",
        declarations.join(", "),
        organization.entity_type(),
        attributes.join(", ")
    );
    let mut rows = GivenRows::new(variables, 1);
    rows.push_row(values)?;
    Ok((query, rows))
}

fn publication_venue_insert_query(
    venue: &dyn TPublicationVenue,
) -> eros::Result<(String, GivenRows)> {
    let mut variables = vec!["venue_id".to_owned(), "venue_name".to_owned()];
    let mut declarations = vec!["$venue_id: string", "$venue_name: string"];
    let mut attributes = vec![
        "has venue_id == $venue_id".to_owned(),
        "has venue_name == $venue_name".to_owned(),
    ];
    let mut values = vec![
        venue.venue_id().to_owned().into(),
        venue.venue_name().to_owned().into(),
    ];
    if let Some(issn) = venue.issn() {
        variables.push("issn".to_owned());
        declarations.push("$issn: string");
        attributes.push("has issn == $issn".to_owned());
        values.push(issn.to_owned().into());
    }
    let query = format!(
        "given {}; insert $venue isa {}, {};",
        declarations.join(", "),
        venue.entity_type(),
        attributes.join(", ")
    );
    let mut rows = GivenRows::new(variables, 1);
    rows.push_row(values)?;
    Ok((query, rows))
}

fn affiliation_insert_query(affiliation: &dyn TAffiliation) -> eros::Result<(String, GivenRows)> {
    let mut variables = vec!["person_id".to_owned(), "organization_id".to_owned()];
    let mut declarations = vec![
        "$person_id: string".to_owned(),
        "$organization_id: string".to_owned(),
    ];
    let mut matches = vec![
        "$person isa person, has person_id == $person_id".to_owned(),
        "$organization isa organization, has organization_id == $organization_id".to_owned(),
    ];
    let mut roles = vec![
        "person: $person".to_owned(),
        "organization: $organization".to_owned(),
    ];
    let mut values = vec![
        affiliation.person().person_id().to_owned().into(),
        affiliation
            .organization()
            .organization_id()
            .to_owned()
            .into(),
    ];
    for (index, evidence) in affiliation.evidence().iter().enumerate() {
        let id = format!("evidence_id_{index}");
        let entity = format!("evidence_{index}");
        variables.push(id.clone());
        declarations.push(format!("${id}: string"));
        matches.push(format!("${entity} isa document, has document_id == ${id}"));
        roles.push(format!("evidence: ${entity}"));
        values.push(evidence.document_id().to_owned().into());
    }
    let query = format!(
        "given {}; match {}; insert $affiliation isa affiliation, links ({});",
        declarations.join(", "),
        matches.join("; "),
        roles.join(", ")
    );
    let mut rows = GivenRows::new(variables, 1);
    rows.push_row(values)?;
    Ok((query, rows))
}

fn publication_event_insert_query(
    event: &dyn TPublicationEvent,
) -> eros::Result<(String, GivenRows)> {
    let mut variables = vec!["document_id".to_owned(), "publication_date".to_owned()];
    let mut declarations = vec![
        "$document_id: string".to_owned(),
        "$publication_date: datetime".to_owned(),
    ];
    let mut matches = vec!["$document isa document, has document_id == $document_id".to_owned()];
    let mut roles = vec!["work: $document".to_owned()];
    let mut attributes = vec!["has publication_date == $publication_date".to_owned()];
    let mut values = vec![
        event.work().document_id().to_owned().into(),
        event.publication_date().into(),
    ];
    if let Some(publisher) = event.publisher() {
        variables.push("publisher_id".to_owned());
        declarations.push("$publisher_id: string".to_owned());
        matches
            .push("$publisher isa organization, has organization_id == $publisher_id".to_owned());
        roles.push("publisher: $publisher".to_owned());
        values.push(publisher.organization_id().to_owned().into());
    }
    if let Some(venue) = event.venue() {
        variables.push("venue_id".to_owned());
        declarations.push("$venue_id: string".to_owned());
        matches.push("$venue isa publication_venue, has venue_id == $venue_id".to_owned());
        roles.push("venue: $venue".to_owned());
        values.push(venue.venue_id().to_owned().into());
    }
    for (index, note) in event.publication_notes().iter().enumerate() {
        let variable = format!("publication_note_{index}");
        variables.push(variable.clone());
        declarations.push(format!("${variable}: string"));
        attributes.push(format!("has publication_note == ${variable}"));
        values.push(note.clone().into());
    }
    if let Some(version_number) = event.version_number() {
        variables.push("version_number".to_owned());
        declarations.push("$version_number: string".to_owned());
        attributes.push("has version_number == $version_number".to_owned());
        values.push(version_number.to_owned().into());
    }
    let query = format!(
        "given {}; match {}; insert $event isa {}, links ({}), {};",
        declarations.join(", "),
        matches.join("; "),
        event.relation_type(),
        roles.join(", "),
        attributes.join(", ")
    );
    let mut rows = GivenRows::new(variables, 1);
    rows.push_row(values)?;
    Ok((query, rows))
}

fn document_delete_query(document: &Arc<dyn TDocument>) -> eros::Result<(String, GivenRows)> {
    let mut rows = GivenRows::new(vec!["document_id".to_owned()], 1);
    rows.push_row(vec![document.document_id().to_owned().into()])?;
    Ok((
        "given $document_id: string; match $document isa document, has document_id == $document_id; delete $document;".to_owned(),
        rows,
    ))
}

fn document_relation_delete_query(
    document: &Arc<dyn TDocument>,
    relation_type: &str,
    role: &str,
) -> eros::Result<(String, GivenRows)> {
    let mut rows = GivenRows::new(vec!["document_id".to_owned()], 1);
    rows.push_row(vec![document.document_id().to_owned().into()])?;
    Ok((
        format!(
            "given $document_id: string; \
             match $document isa document, has document_id == $document_id; \
             $relation isa {relation_type}, links ({role}: $document); delete $relation;"
        ),
        rows,
    ))
}

fn organization_delete_query(
    organization: &Arc<dyn TOrganization>,
) -> eros::Result<(String, GivenRows)> {
    let mut rows = GivenRows::new(vec!["organization_id".to_owned()], 1);
    rows.push_row(vec![organization.organization_id().to_owned().into()])?;
    Ok((
        "given $organization_id: string; \
         match $organization isa organization, has organization_id == $organization_id; \
         delete $organization;"
            .to_owned(),
        rows,
    ))
}

fn publication_venue_delete_query(
    venue: &Arc<dyn TPublicationVenue>,
) -> eros::Result<(String, GivenRows)> {
    let mut rows = GivenRows::new(vec!["venue_id".to_owned()], 1);
    rows.push_row(vec![venue.venue_id().to_owned().into()])?;
    Ok((
        "given $venue_id: string; \
         match $venue isa publication_venue, has venue_id == $venue_id; delete $venue;"
            .to_owned(),
        rows,
    ))
}

fn document_value(document: &Arc<dyn TDocument>) -> Option<serde_json::Value> {
    serde_json::to_value(document).ok()
}

#[cfg(test)]
fn document_title_update_query(
    old: &Arc<dyn TDocument>,
    new: &Arc<dyn TDocument>,
) -> eros::Result<(String, GivenRows)> {
    let mut rows = GivenRows::new(
        vec![
            "document_id".to_owned(),
            "old_title".to_owned(),
            "new_title".to_owned(),
        ],
        1,
    );
    rows.push_row(vec![
        old.document_id().to_owned().into(),
        old.title().to_owned().into(),
        new.title().to_owned().into(),
    ])?;
    Ok((
        "given $document_id: string, $old_title: string, $new_title: string; \
         match $document isa document, has document_id == $document_id, has title == $old_title; \
         delete has $old_title of $document; \
         insert $document has title == $new_title;"
            .to_owned(),
        rows,
    ))
}

fn person_delete_query(person: &dyn TPerson) -> eros::Result<(String, GivenRows)> {
    let mut rows = GivenRows::new(vec!["person_id".to_owned()], 1);
    rows.push_row(vec![person.person_id().to_owned().into()])?;
    Ok((
        "given $person_id: string; match $person isa person, has person_id == $person_id; delete $person;"
            .to_owned(),
        rows,
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::models::draft::{
        Bibliography, Contributor, ContributorRole, Identifier, IdentifierKind, IdentifierScope,
        PassageLevel,
    };

    #[derive(Clone, Default)]
    struct RecordingStore(Arc<Mutex<Vec<CanonicalModel>>>);

    #[async_trait]
    impl CanonicalDocumentStore for RecordingStore {
        async fn insert(&self, model: &CanonicalModel) -> eros::Result<()> {
            self.0.lock().unwrap().push(model.clone());
            Ok(())
        }

        async fn update(
            &self,
            old: &CanonicalModel,
            new: &CanonicalModel,
        ) -> eros::Result<CanonicalUpdateSummary> {
            if old == new {
                return Ok(CanonicalUpdateSummary::default());
            }
            Ok(CanonicalUpdateSummary {
                document_changed: document_value(&old.document) != document_value(&new.document),
                contributors_deleted: old.persons.len(),
                contributors_inserted: new.persons.len(),
                organizations_deleted: old.organizations.len(),
                organizations_inserted: new.organizations.len(),
                venues_deleted: old.publication_venues.len(),
                venues_inserted: new.publication_venues.len(),
                affiliations_deleted: old.affiliations.len(),
                affiliations_inserted: new.affiliations.len(),
                publication_events_deleted: old.publication_events.len(),
                publication_events_inserted: new.publication_events.len(),
            })
        }
    }

    fn draft() -> TeiDocument {
        TeiDocument {
            level: PassageLevel::Paragraph,
            bibliography: Bibliography {
                title: Some("Canonical title".to_owned()),
                authors: vec![Contributor {
                    name: "Ada Lovelace".to_owned(),
                    forename: Some("Ada".to_owned()),
                    surname: Some("Lovelace".to_owned()),
                    affiliation: None,
                    role: ContributorRole::Author,
                }],
                identifiers: vec![Identifier {
                    kind: IdentifierKind::Doi,
                    value: "10.1234/canonical".to_owned(),
                    scope: IdentifierScope::Document,
                }],
                ..Bibliography::default()
            },
            body_text: vec![],
            figures_and_tables: vec![],
            references: vec![],
        }
    }

    #[tokio::test]
    async fn execution_inserts_the_model_returned_by_pre_validation() {
        let store = RecordingStore::default();
        let inserted = store.0.clone();
        let service = TypeDbService::new(store);

        let canonical = service.pre_validate(&draft()).await.unwrap();
        service.execute(&canonical).await.unwrap();

        assert_eq!(inserted.lock().unwrap().as_slice(), &[canonical]);
    }

    #[tokio::test]
    async fn run_does_not_execute_when_canonicalisation_fails() {
        let store = RecordingStore::default();
        let inserted = store.0.clone();
        let service = TypeDbService::new(store);
        let mut invalid = draft();
        invalid.bibliography.title = None;

        assert!(service.run(&invalid).await.is_err());
        assert!(inserted.lock().unwrap().is_empty());
    }

    #[test]
    fn research_paper_insert_uses_typed_parameters() {
        let canonical = CanonicalModel::try_from(&draft()).unwrap();
        let (query, rows) = document_insert_query(&canonical.document).unwrap();
        let (_, values) = rows.into_parts();

        assert!(query.contains("isa research_paper"));
        assert!(query.contains("has doi == $doi"));
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].len(), 3);
    }

    #[test]
    fn contribution_insert_links_person_to_document() {
        let canonical = CanonicalModel::try_from(&draft()).unwrap();
        let (query, rows) = contribution_insert_query(canonical.contributions[0].as_ref()).unwrap();
        let (_, values) = rows.into_parts();

        assert!(query.contains("isa authorship"));
        assert!(query.contains("links (contributor: $contributor, work: $document)"));
        assert_eq!(values[0].len(), 2);
    }

    #[test]
    fn expanded_graph_builds_typedb_queries_for_every_extracted_model() {
        let canonical = expanded_canonical();

        let (organization_query, _) =
            organization_insert_query(canonical.organizations[0].as_ref()).unwrap();
        let (affiliation_query, _) =
            affiliation_insert_query(canonical.affiliations[0].as_ref()).unwrap();
        let (venue_query, _) =
            publication_venue_insert_query(canonical.publication_venues[0].as_ref()).unwrap();
        let (event_query, event_rows) =
            publication_event_insert_query(canonical.publication_events[0].as_ref()).unwrap();
        let (_, event_values) = event_rows.into_parts();

        assert!(organization_query.contains("isa organization"));
        assert!(organization_query.contains("has organization_name"));
        assert!(affiliation_query.contains("isa affiliation"));
        assert!(affiliation_query.contains("evidence: $evidence_0"));
        assert!(venue_query.contains("isa journal"));
        assert!(venue_query.contains("has issn"));
        assert!(event_query.contains("isa publication"));
        assert!(event_query.contains("publisher: $publisher"));
        assert!(event_query.contains("venue: $venue"));
        assert_eq!(event_values[0].len(), 4);
    }

    fn expanded_canonical() -> CanonicalModel {
        CanonicalModel::try_from(&expanded_draft()).unwrap()
    }

    fn expanded_draft() -> TeiDocument {
        let mut draft = draft();
        draft.bibliography.authors[0].affiliation = Some("Example University".into());
        draft.bibliography.publisher = Some("Example Press".into());
        draft.bibliography.journal = Some("Example Journal".into());
        draft.bibliography.publication_date = Some("2024-05-06".into());
        draft.bibliography.publication_year = Some(2024);
        draft.bibliography.identifiers.push(Identifier {
            kind: IdentifierKind::Issn,
            value: "1234-5678".into(),
            scope: IdentifierScope::Document,
        });
        draft
    }

    #[tokio::test]
    #[ignore = "requires a local TypeDB service"]
    async fn live_schema_accepts_expanded_graph_queries() {
        let (address, database, username, password) = live_typedb_settings();
        let driver = TypeDBDriver::new(
            Addresses::try_from_address_str(&address).unwrap(),
            Credentials::new(&username, &password),
            DriverOptions::new(DriverTlsConfig::disabled()),
        )
        .await
        .unwrap();
        let transaction = driver
            .transaction(&database, TransactionType::Read)
            .await
            .unwrap();
        let canonical = expanded_canonical();
        let queries = vec![
            document_insert_query(&canonical.document).unwrap().0,
            person_insert_query(canonical.persons[0].as_ref())
                .unwrap()
                .0,
            contribution_insert_query(canonical.contributions[0].as_ref())
                .unwrap()
                .0,
            organization_insert_query(canonical.organizations[0].as_ref())
                .unwrap()
                .0,
            affiliation_insert_query(canonical.affiliations[0].as_ref())
                .unwrap()
                .0,
            publication_venue_insert_query(canonical.publication_venues[0].as_ref())
                .unwrap()
                .0,
            publication_event_insert_query(canonical.publication_events[0].as_ref())
                .unwrap()
                .0,
        ];

        for query in queries {
            transaction.analyze(&query).await.unwrap();
        }
        transaction.close().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires a local TypeDB service and creates a temporary database"]
    async fn expanded_graph_commits_against_a_fresh_schema() {
        let (address, _, username, password) = live_typedb_settings();
        let database = format!("scepa_graph_test_{}", std::process::id());
        let driver = TypeDBDriver::new(
            Addresses::try_from_address_str(&address).unwrap(),
            Credentials::new(&username, &password),
            DriverOptions::new(DriverTlsConfig::disabled()),
        )
        .await
        .unwrap();
        assert!(!driver.databases().contains(&database).await.unwrap());
        let service = TypeDbService::from_driver(driver, &database);
        service.ensure_schema().await.unwrap();

        let old = expanded_canonical();
        let mut changed_draft = expanded_draft();
        changed_draft.bibliography.title = Some("Corrected title".into());
        let new = CanonicalModel::try_from(&changed_draft).unwrap();
        let result = async {
            service.execute(&old).await?;
            service.execute_update(&old, &new).await.map(|_| ())
        }
        .await;
        service
            .store
            .driver
            .databases()
            .get(&database)
            .await
            .unwrap()
            .delete()
            .await
            .unwrap();

        result.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires a local TypeDB service and creates a temporary database"]
    async fn existing_schema_is_migrated_for_non_contributor_organizations() {
        let (address, _, username, password) = live_typedb_settings();
        let database = format!("scepa_schema_migration_test_{}", std::process::id());
        let driver = TypeDBDriver::new(
            Addresses::try_from_address_str(&address).unwrap(),
            Credentials::new(&username, &password),
            DriverOptions::new(DriverTlsConfig::disabled()),
        )
        .await
        .unwrap();
        assert!(!driver.databases().contains(&database).await.unwrap());
        driver.databases().create(&database).await.unwrap();
        let transaction = driver
            .transaction(&database, TransactionType::Schema)
            .await
            .unwrap();
        transaction
            .query(&include_str!("../../schema.tql").replace(
                "plays contribution:contributor @card(0..),",
                "plays contribution:contributor @card(1..),",
            ))
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let service = TypeDbService::from_driver(driver, &database);
        let result = async {
            service.ensure_schema().await?;
            service.execute(&expanded_canonical()).await
        }
        .await;
        service
            .store
            .driver
            .databases()
            .get(&database)
            .await
            .unwrap()
            .delete()
            .await
            .unwrap();

        result.unwrap();
    }

    fn live_typedb_settings() -> (String, String, String, String) {
        let config = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.env"),
        )
        .unwrap();
        let setting = |name: &str| {
            std::env::var(name).ok().or_else(|| {
                config.lines().find_map(|line| {
                    let (key, value) = line.split_once('=')?;
                    (key == name).then(|| value.to_owned())
                })
            })
        };
        (
            setting("TYPEDB_ADDRESS")
                .unwrap()
                .replace("typedb:", "localhost:"),
            setting("TYPEDB_DATABASE").unwrap(),
            setting("TYPEDB_USERNAME").unwrap(),
            setting("TYPEDB_PASSWORD").unwrap(),
        )
    }

    #[tokio::test]
    async fn unchanged_update_executes_no_graph_parts() {
        let service = TypeDbService::new(RecordingStore::default());
        let canonical = CanonicalModel::try_from(&draft()).unwrap();

        let summary = service
            .execute_update(&canonical, &canonical)
            .await
            .unwrap();

        assert_eq!(summary, CanonicalUpdateSummary::default());
    }

    #[test]
    fn delete_queries_target_stable_identifiers() {
        let canonical = CanonicalModel::try_from(&draft()).unwrap();
        let (document_query, _) = document_delete_query(&canonical.document).unwrap();
        let (contribution_query, _) =
            document_relation_delete_query(&canonical.document, "contribution", "work").unwrap();
        let (person_query, _) = person_delete_query(canonical.persons[0].as_ref()).unwrap();

        assert!(document_query.contains("has document_id == $document_id"));
        assert!(contribution_query.contains("delete $relation"));
        assert!(person_query.contains("has person_id == $person_id"));
    }

    #[test]
    fn title_only_update_keeps_document_identity() {
        let old = CanonicalModel::try_from(&draft()).unwrap();
        let mut changed_draft = draft();
        changed_draft.bibliography.title = Some("A corrected title".into());
        let new = CanonicalModel::try_from(&changed_draft).unwrap();
        let (query, _) = document_title_update_query(&old.document, &new.document).unwrap();

        assert_eq!(old.document.document_id(), new.document.document_id());
        assert_eq!(old.document.entity_type(), new.document.entity_type());
        assert!(query.contains("delete has $old_title of $document"));
        assert!(query.contains("insert $document has title == $new_title"));
    }

    #[tokio::test]
    async fn supplied_sha256_is_persisted_in_the_canonical_model() {
        let service = TypeDbService::new(RecordingStore::default());
        let hash = "a".repeat(64);
        let canonical = service
            .pre_validate_with_pdf_hash(&draft(), &hash)
            .await
            .unwrap();

        assert_eq!(canonical.document.pdf_hash(), Some(hash.as_str()));
    }

    #[tokio::test]
    async fn supplied_sha256_is_the_fallback_canonical_identifier() {
        let service = TypeDbService::new(RecordingStore::default());
        let hash = "b".repeat(64);
        let mut draft = draft();
        draft.bibliography.identifiers.clear();

        let canonical = service
            .pre_validate_with_pdf_hash(&draft, &hash)
            .await
            .unwrap();

        assert_eq!(canonical.document.document_id(), format!("sha256:{hash}"));
        assert_eq!(canonical.document.pdf_hash(), Some(hash.as_str()));
        assert_eq!(canonical.document.entity_type(), "document");
    }

    #[tokio::test]
    async fn stable_identifier_takes_precedence_over_supplied_sha256() {
        let service = TypeDbService::new(RecordingStore::default());
        let hash = "c".repeat(64);

        let canonical = service
            .pre_validate_with_pdf_hash(&draft(), &hash)
            .await
            .unwrap();

        assert_eq!(canonical.document.document_id(), "10.1234/canonical");
        assert_eq!(canonical.document.pdf_hash(), Some(hash.as_str()));
    }
}
