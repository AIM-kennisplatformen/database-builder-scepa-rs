//! Service for validating draft documents and persisting canonical documents.

use std::sync::Arc;

use async_trait::async_trait;
use typedb_driver::{
    Addresses, Credentials, DriverOptions, DriverTlsConfig, TransactionType, TypeDBDriver,
    given::GivenRows,
};

use crate::models::{
    canonical::{CanonicalContribution, CanonicalContributor, CanonicalDocument, CanonicalModel},
    draft::TeiDocument,
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
}

impl CanonicalUpdateSummary {
    pub fn changed(&self) -> bool {
        self.document_changed || self.contributors_deleted != 0 || self.contributors_inserted != 0
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
                 $authorship label authorship; \
                 $contribution label contribution; \
                 $document_id label document_id; \
                 $pdf_hash label pdf_hash; \
                 $person_id label person_id; \
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
        for contributor in &model.contributors {
            let (query, rows) = contribution_insert_query(&model.document, contributor)?;
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
        let document_changed = old.document != new.document;
        let replace_document = !document_identity_equal(&old.document, &new.document);
        let deleted = if replace_document {
            old.contributors.iter().collect::<Vec<_>>()
        } else {
            old.contributors
                .iter()
                .filter(|contributor| !new.contributors.contains(contributor))
                .collect()
        };
        let inserted = if replace_document {
            new.contributors.iter().collect::<Vec<_>>()
        } else {
            new.contributors
                .iter()
                .filter(|contributor| !old.contributors.contains(contributor))
                .collect()
        };
        let summary = CanonicalUpdateSummary {
            document_changed,
            contributors_deleted: deleted.len(),
            contributors_inserted: inserted.len(),
        };
        if !summary.changed() {
            return Ok(summary);
        }

        let transaction = self
            .driver
            .transaction(&self.database, TransactionType::Write)
            .await?;

        for contributor in deleted {
            let (query, rows) = contribution_delete_query(&old.document, contributor)?;
            transaction.query_with_rows(query, rows).await?;
        }
        if replace_document {
            let (query, rows) = document_delete_query(&old.document)?;
            transaction.query_with_rows(query, rows).await?;
            let (query, rows) = document_insert_query(&new.document)?;
            transaction.query_with_rows(query, rows).await?;
        } else if document_changed {
            let (query, rows) = document_title_update_query(&old.document, &new.document)?;
            transaction.query_with_rows(query, rows).await?;
        }
        for contributor in inserted {
            let (query, rows) = contribution_insert_query(&new.document, contributor)?;
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

fn document_insert_query(document: &CanonicalDocument) -> eros::Result<(String, GivenRows)> {
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

    match document {
        CanonicalDocument::ResearchPaper(document) => {
            if let Some(doi) = &document.doi {
                variables.push("doi".to_owned());
                declarations.push("$doi: string");
                attributes.push("has doi == $doi".to_owned());
                values.push(doi.clone().into());
            }
        }
        CanonicalDocument::Book(document) => {
            if let Some(isbn) = &document.isbn {
                variables.push("isbn".to_owned());
                declarations.push("$isbn: string");
                attributes.push("has isbn == $isbn".to_owned());
                values.push(isbn.clone().into());
            }
        }
        CanonicalDocument::Document(_) => {}
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

fn contribution_insert_query(
    document: &CanonicalDocument,
    contributor: &CanonicalContributor,
) -> eros::Result<(String, GivenRows)> {
    let mut variables = vec!["document_id".to_owned(), "person_id".to_owned()];
    let mut declarations = vec!["$document_id: string", "$person_id: string"];
    let mut person_attributes = vec!["has person_id == $person_id".to_owned()];
    let mut values = vec![
        document.document_id().to_owned().into(),
        contributor.person.person_id.clone().into(),
    ];

    if let Some(given_name) = &contributor.person.given_name {
        variables.push("given_name".to_owned());
        declarations.push("$given_name: string");
        person_attributes.push("has given_name == $given_name".to_owned());
        values.push(given_name.clone().into());
    }
    if let Some(family_name) = &contributor.person.family_name {
        variables.push("family_name".to_owned());
        declarations.push("$family_name: string");
        person_attributes.push("has family_name == $family_name".to_owned());
        values.push(family_name.clone().into());
    }

    let relation_type = match contributor.contribution {
        CanonicalContribution::Authorship => "authorship",
        CanonicalContribution::Contribution => "contribution",
    };
    let query = format!(
        "given {}; match $document isa document, has document_id == $document_id; \
         insert $person isa person, {}; \
         $contribution isa {}, links (contributor: $person, work: $document);",
        declarations.join(", "),
        person_attributes.join(", "),
        relation_type,
    );
    let mut rows = GivenRows::new(variables, 1);
    rows.push_row(values)?;
    Ok((query, rows))
}

fn document_delete_query(document: &CanonicalDocument) -> eros::Result<(String, GivenRows)> {
    let mut rows = GivenRows::new(vec!["document_id".to_owned()], 1);
    rows.push_row(vec![document.document_id().to_owned().into()])?;
    Ok((
        "given $document_id: string; match $document isa document, has document_id == $document_id; delete $document;".to_owned(),
        rows,
    ))
}

fn document_identity_equal(old: &CanonicalDocument, new: &CanonicalDocument) -> bool {
    old.entity_type() == new.entity_type()
        && old.document_id() == new.document_id()
        && old.pdf_hash() == new.pdf_hash()
        && match (old, new) {
            (CanonicalDocument::ResearchPaper(old), CanonicalDocument::ResearchPaper(new)) => {
                old.doi == new.doi
            }
            (CanonicalDocument::Book(old), CanonicalDocument::Book(new)) => old.isbn == new.isbn,
            (CanonicalDocument::Document(_), CanonicalDocument::Document(_)) => true,
            _ => false,
        }
}

fn document_title_update_query(
    old: &CanonicalDocument,
    new: &CanonicalDocument,
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

fn contribution_delete_query(
    document: &CanonicalDocument,
    contributor: &CanonicalContributor,
) -> eros::Result<(String, GivenRows)> {
    let relation_type = match contributor.contribution {
        CanonicalContribution::Authorship => "authorship",
        CanonicalContribution::Contribution => "contribution",
    };
    let query = format!(
        "given $document_id: string, $person_id: string; \
         match $document isa document, has document_id == $document_id; \
         $person isa person, has person_id == $person_id; \
         $contribution isa {relation_type}, links (contributor: $person, work: $document); \
         delete $contribution; delete $person;"
    );
    let mut rows = GivenRows::new(vec!["document_id".to_owned(), "person_id".to_owned()], 1);
    rows.push_row(vec![
        document.document_id().to_owned().into(),
        contributor.person.person_id.clone().into(),
    ])?;
    Ok((query, rows))
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
            let document_changed = old.document != new.document;
            let replace_document = !document_identity_equal(&old.document, &new.document);
            Ok(CanonicalUpdateSummary {
                document_changed,
                contributors_deleted: old
                    .contributors
                    .iter()
                    .filter(|row| replace_document || !new.contributors.contains(row))
                    .count(),
                contributors_inserted: new
                    .contributors
                    .iter()
                    .filter(|row| replace_document || !old.contributors.contains(row))
                    .count(),
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
        let (query, rows) =
            contribution_insert_query(&canonical.document, &canonical.contributors[0]).unwrap();
        let (_, values) = rows.into_parts();

        assert!(query.contains("isa authorship"));
        assert!(query.contains("links (contributor: $person, work: $document)"));
        assert_eq!(values[0].len(), 4);
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
            contribution_delete_query(&canonical.document, &canonical.contributors[0]).unwrap();

        assert!(document_query.contains("has document_id == $document_id"));
        assert!(contribution_query.contains("has person_id == $person_id"));
        assert!(contribution_query.contains("delete $contribution"));
    }

    #[test]
    fn title_only_update_keeps_document_identity() {
        let old = CanonicalModel::try_from(&draft()).unwrap();
        let mut changed_draft = draft();
        changed_draft.bibliography.title = Some("A corrected title".into());
        let new = CanonicalModel::try_from(&changed_draft).unwrap();
        let (query, _) = document_title_update_query(&old.document, &new.document).unwrap();

        assert!(document_identity_equal(&old.document, &new.document));
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
        assert!(matches!(canonical.document, CanonicalDocument::Document(_)));
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
