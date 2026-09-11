use std::{collections::BTreeMap, sync::Arc};

use futures::TryStreamExt;
use typedb_driver::{
    Addresses, Credentials, DriverOptions, DriverTlsConfig, TransactionType, TypeDBDriver,
    answer::concept_row::ConceptRow, concept::Concept, given::GivenRows,
};

use crate::{
    models::{
        AffiliationMetadata, ContributionMetadata, DocumentMetadata, LiteratureFilters,
        OrganizationRoleFilter, PartyMetadata, PublicationEventMetadata, VenueMetadata,
    },
    search::SearchError,
};

#[derive(Clone)]
pub struct MetadataStore {
    driver: Arc<TypeDBDriver>,
    database: String,
}

impl MetadataStore {
    pub async fn connect(
        address: &str,
        database: String,
        username: &str,
        password: &str,
    ) -> Result<Self, SearchError> {
        let addresses = Addresses::try_from_address_str(address).map_err(typedb_error)?;
        let driver = TypeDBDriver::new(
            addresses,
            Credentials::new(username, password),
            DriverOptions::new(DriverTlsConfig::disabled()),
        )
        .await
        .map_err(typedb_error)?;
        if !driver
            .databases()
            .contains(&database)
            .await
            .map_err(typedb_error)?
        {
            return Err(SearchError::TypeDb(format!(
                "database `{database}` does not exist"
            )));
        }
        Ok(Self {
            driver: Arc::new(driver),
            database,
        })
    }

    pub async fn eligible_pdf_hashes(
        &self,
        filters: &LiteratureFilters,
    ) -> Result<Vec<String>, SearchError> {
        validate_filters(filters)?;
        let rows = self.read_rows(&filter_query(filters), None).await?;
        let mut hashes = rows
            .iter()
            .map(|row| row_string(row, "pdf_hash"))
            .collect::<Result<Vec<_>, _>>()?;
        hashes.sort();
        hashes.dedup();
        Ok(hashes)
    }

    pub async fn document_metadata(
        &self,
        pdf_hashes: &[String],
    ) -> Result<BTreeMap<String, DocumentMetadata>, SearchError> {
        if pdf_hashes.is_empty() {
            return Ok(BTreeMap::new());
        }
        let mut documents = BTreeMap::new();
        for row in self
            .metadata_rows(
                "given $requested_hash: string; match \
                 $document isa! $document_type, has pdf_hash == $requested_hash, \
                    has document_id $document_id, has title $title; \
                 select $requested_hash, $document_type, $document_id, $title;",
                pdf_hashes,
            )
            .await?
        {
            let hash = row_string(&row, "requested_hash")?;
            documents.insert(
                hash.clone(),
                DocumentMetadata {
                    pdf_hash: hash,
                    document_id: row_string(&row, "document_id")?,
                    document_type: row_label(&row, "document_type")?,
                    title: row_string(&row, "title")?,
                    ieee_reference: String::new(),
                    doi: None,
                    isbn: Vec::new(),
                    persons: Vec::new(),
                    organizations: Vec::new(),
                    contributors: Vec::new(),
                    affiliations: Vec::new(),
                    publication_events: Vec::new(),
                },
            );
        }
        self.add_identifiers(pdf_hashes, &mut documents).await?;
        let parties = self.parties(pdf_hashes).await?;
        for ((hash, _), party) in &parties {
            if let Some(document) = documents.get_mut(hash) {
                if party.entity_type == "person" {
                    document.persons.push(party.clone());
                } else {
                    document.organizations.push(party.clone());
                }
            }
        }
        self.add_contributions(pdf_hashes, &parties, &mut documents)
            .await?;
        self.add_affiliations(pdf_hashes, &mut documents).await?;
        self.add_publications(pdf_hashes, &parties, &mut documents)
            .await?;
        for document in documents.values_mut() {
            document.refresh_ieee_reference();
        }
        Ok(documents)
    }

    async fn read_rows(
        &self,
        query: &str,
        inputs: Option<GivenRows>,
    ) -> Result<Vec<ConceptRow>, SearchError> {
        let transaction = self
            .driver
            .transaction(&self.database, TransactionType::Read)
            .await
            .map_err(typedb_error)?;
        let answer = match inputs {
            Some(rows) => transaction.query_with_rows(query, rows).await,
            None => transaction.query(query).await,
        }
        .map_err(typedb_error)?;
        let rows = answer
            .into_rows()
            .try_collect()
            .await
            .map_err(typedb_error)?;
        transaction.close().await.map_err(typedb_error)?;
        Ok(rows)
    }

    async fn metadata_rows(
        &self,
        query: &str,
        hashes: &[String],
    ) -> Result<Vec<ConceptRow>, SearchError> {
        let mut inputs = GivenRows::new(vec!["requested_hash".into()], hashes.len());
        for hash in hashes {
            inputs
                .push_row(vec![hash.clone().into()])
                .map_err(typedb_error)?;
        }
        self.read_rows(query, Some(inputs)).await
    }

    async fn add_identifiers(
        &self,
        hashes: &[String],
        documents: &mut BTreeMap<String, DocumentMetadata>,
    ) -> Result<(), SearchError> {
        for attribute in ["doi", "isbn"] {
            let query = format!(
                "given $requested_hash: string; match $document isa document, \
                 has pdf_hash == $requested_hash, has {attribute} $value; \
                 select $requested_hash, $value;"
            );
            for row in self.metadata_rows(&query, hashes).await? {
                let hash = row_string(&row, "requested_hash")?;
                let value = row_string(&row, "value")?;
                if let Some(document) = documents.get_mut(&hash) {
                    if attribute == "doi" {
                        document.doi = Some(value);
                    } else {
                        document.isbn.push(value);
                    }
                }
            }
        }
        Ok(())
    }

    async fn parties(
        &self,
        hashes: &[String],
    ) -> Result<BTreeMap<(String, String), PartyMetadata>, SearchError> {
        let mut parties = BTreeMap::new();
        let people = "given $requested_hash: string; match \
            $document isa document, has pdf_hash == $requested_hash; \
            $party isa! $party_type, has person_id $party_id; \
            { $relation isa contribution, links (work: $document, contributor: $party); } or \
            { $relation isa affiliation, links (evidence: $document, person: $party); }; \
            select $requested_hash, $party_type, $party_id;";
        for row in self.metadata_rows(people, hashes).await? {
            let hash = row_string(&row, "requested_hash")?;
            let id = row_string(&row, "party_id")?;
            parties.entry((hash, id.clone())).or_insert(PartyMetadata {
                id,
                entity_type: row_label(&row, "party_type")?,
                given_name: None,
                family_name: None,
                organization_name: None,
                ror_id: None,
            });
        }
        for (attribute, given_name) in [("given_name", true), ("family_name", false)] {
            let query = format!(
                "given $requested_hash: string; match \
                 $document isa document, has pdf_hash == $requested_hash; \
                 $party isa person, has person_id $party_id, has {attribute} $name; \
                 {{ $relation isa contribution, links (work: $document, contributor: $party); }} or \
                 {{ $relation isa affiliation, links (evidence: $document, person: $party); }}; \
                 select $requested_hash, $party_id, $name;"
            );
            for row in self.metadata_rows(&query, hashes).await? {
                let key = (
                    row_string(&row, "requested_hash")?,
                    row_string(&row, "party_id")?,
                );
                if let Some(party) = parties.get_mut(&key) {
                    if given_name {
                        party.given_name = Some(row_string(&row, "name")?);
                    } else {
                        party.family_name = Some(row_string(&row, "name")?);
                    }
                }
            }
        }

        let organizations = "given $requested_hash: string; match \
            $document isa document, has pdf_hash == $requested_hash; \
            $party isa! $party_type, has organization_id $party_id, has organization_name $name; \
            { $relation isa contribution, links (work: $document, contributor: $party); } or \
            { $relation isa affiliation, links (evidence: $document, organization: $party); } or \
            { $relation isa publication_event, links (work: $document, publisher: $party); }; \
            select $requested_hash, $party_type, $party_id, $name;";
        for row in self.metadata_rows(organizations, hashes).await? {
            let hash = row_string(&row, "requested_hash")?;
            let id = row_string(&row, "party_id")?;
            parties.insert(
                (hash, id.clone()),
                PartyMetadata {
                    id,
                    entity_type: row_label(&row, "party_type")?,
                    given_name: None,
                    family_name: None,
                    organization_name: Some(row_string(&row, "name")?),
                    ror_id: None,
                },
            );
        }
        let rors = "given $requested_hash: string; match \
            $document isa document, has pdf_hash == $requested_hash; \
            $party isa organization, has organization_id $party_id, has ror_id $ror_id; \
            { $relation isa contribution, links (work: $document, contributor: $party); } or \
            { $relation isa affiliation, links (evidence: $document, organization: $party); } or \
            { $relation isa publication_event, links (work: $document, publisher: $party); }; \
            select $requested_hash, $party_id, $ror_id;";
        for row in self.metadata_rows(rors, hashes).await? {
            let key = (
                row_string(&row, "requested_hash")?,
                row_string(&row, "party_id")?,
            );
            if let Some(party) = parties.get_mut(&key) {
                party.ror_id = Some(row_string(&row, "ror_id")?);
            }
        }
        Ok(parties)
    }

    async fn add_contributions(
        &self,
        hashes: &[String],
        parties: &BTreeMap<(String, String), PartyMetadata>,
        documents: &mut BTreeMap<String, DocumentMetadata>,
    ) -> Result<(), SearchError> {
        let query = "given $requested_hash: string; match \
            $document isa document, has pdf_hash == $requested_hash; \
            $relation isa! $relation_type, links (work: $document, contributor: $party); \
            { $party isa person, has person_id $party_id; } or \
            { $party isa organization, has organization_id $party_id; }; \
            select $requested_hash, $relation_type, $party_id;";
        for row in self.metadata_rows(query, hashes).await? {
            let hash = row_string(&row, "requested_hash")?;
            let id = row_string(&row, "party_id")?;
            if let (Some(document), Some(party)) =
                (documents.get_mut(&hash), parties.get(&(hash.clone(), id)))
            {
                document.contributors.push(ContributionMetadata {
                    contribution_type: row_label(&row, "relation_type")?,
                    contributor: party.clone(),
                });
            }
        }
        Ok(())
    }

    async fn add_affiliations(
        &self,
        hashes: &[String],
        documents: &mut BTreeMap<String, DocumentMetadata>,
    ) -> Result<(), SearchError> {
        let query = "given $requested_hash: string; match \
            $document isa document, has pdf_hash == $requested_hash; \
            $relation isa affiliation, links \
                (evidence: $document, person: $person, organization: $organization); \
            $person has person_id $person_id; $organization has organization_id $organization_id; \
            select $requested_hash, $person_id, $organization_id;";
        for row in self.metadata_rows(query, hashes).await? {
            let hash = row_string(&row, "requested_hash")?;
            if let Some(document) = documents.get_mut(&hash) {
                document.affiliations.push(AffiliationMetadata {
                    person_id: row_string(&row, "person_id")?,
                    organization_id: row_string(&row, "organization_id")?,
                });
            }
        }
        Ok(())
    }

    async fn add_publications(
        &self,
        hashes: &[String],
        parties: &BTreeMap<(String, String), PartyMetadata>,
        documents: &mut BTreeMap<String, DocumentMetadata>,
    ) -> Result<(), SearchError> {
        let events = "given $requested_hash: string; match \
            $document isa document, has pdf_hash == $requested_hash; \
            $event isa! $event_type, links (work: $document), has publication_date $date; \
            select $requested_hash, $event_type, $date;";
        for row in self.metadata_rows(events, hashes).await? {
            let hash = row_string(&row, "requested_hash")?;
            if let Some(document) = documents.get_mut(&hash) {
                document.publication_events.push(PublicationEventMetadata {
                    event_type: row_label(&row, "event_type")?,
                    publication_date: row_datetime(&row, "date")?,
                    publication_notes: Vec::new(),
                    version_number: None,
                    publisher: None,
                    venue: None,
                });
            }
        }
        let publishers = "given $requested_hash: string; match \
            $document isa document, has pdf_hash == $requested_hash; \
            $event isa! $event_type, links (work: $document, publisher: $publisher), \
                has publication_date $date; $publisher has organization_id $publisher_id; \
            select $requested_hash, $event_type, $date, $publisher_id;";
        for row in self.metadata_rows(publishers, hashes).await? {
            let hash = row_string(&row, "requested_hash")?;
            let event_type = row_label(&row, "event_type")?;
            let date = row_datetime(&row, "date")?;
            let publisher_id = row_string(&row, "publisher_id")?;
            if let Some(event) = find_event(documents, &hash, &event_type, &date) {
                event.publisher = parties.get(&(hash, publisher_id)).cloned();
            }
        }
        let venues = "given $requested_hash: string; match \
            $document isa document, has pdf_hash == $requested_hash; \
            $event isa! $event_type, links (work: $document, venue: $venue), has publication_date $date; \
            $venue isa! $venue_type, has venue_id $venue_id, has venue_name $venue_name; \
            select $requested_hash, $event_type, $date, $venue_type, $venue_id, $venue_name;";
        for row in self.metadata_rows(venues, hashes).await? {
            let hash = row_string(&row, "requested_hash")?;
            let event_type = row_label(&row, "event_type")?;
            let date = row_datetime(&row, "date")?;
            if let Some(event) = find_event(documents, &hash, &event_type, &date) {
                event.venue = Some(VenueMetadata {
                    id: row_string(&row, "venue_id")?,
                    venue_type: row_label(&row, "venue_type")?,
                    name: row_string(&row, "venue_name")?,
                    issn: None,
                });
            }
        }
        let venue_issns = "given $requested_hash: string; match \
            $document isa document, has pdf_hash == $requested_hash; \
            $event isa! $event_type, links (work: $document, venue: $venue), has publication_date $date; \
            $venue has venue_id $venue_id, has issn $issn; \
            select $requested_hash, $event_type, $date, $venue_id, $issn;";
        for row in self.metadata_rows(venue_issns, hashes).await? {
            let hash = row_string(&row, "requested_hash")?;
            let event_type = row_label(&row, "event_type")?;
            let date = row_datetime(&row, "date")?;
            let venue_id = row_string(&row, "venue_id")?;
            if let Some(venue) = find_event(documents, &hash, &event_type, &date)
                .and_then(|event| event.venue.as_mut())
                .filter(|venue| venue.id == venue_id)
            {
                venue.issn = Some(row_string(&row, "issn")?);
            }
        }
        for attribute in ["publication_note", "version_number"] {
            let query = format!(
                "given $requested_hash: string; match $document isa document, \
                 has pdf_hash == $requested_hash; $event isa! $event_type, \
                 links (work: $document), has publication_date $date, has {attribute} $value; \
                 select $requested_hash, $event_type, $date, $value;"
            );
            for row in self.metadata_rows(&query, hashes).await? {
                let hash = row_string(&row, "requested_hash")?;
                let event_type = row_label(&row, "event_type")?;
                let date = row_datetime(&row, "date")?;
                let value = row_string(&row, "value")?;
                if let Some(event) = find_event(documents, &hash, &event_type, &date) {
                    if attribute == "publication_note" {
                        event.publication_notes.push(value);
                    } else {
                        event.version_number = Some(value);
                    }
                }
            }
        }
        Ok(())
    }
}

pub fn validate_filters(filters: &LiteratureFilters) -> Result<(), SearchError> {
    if let Some(range) = &filters.publication_date
        && let (Some(from), Some(to)) = (range.from, range.to)
        && from > to
    {
        return Err(SearchError::InvalidInput(format!(
            "publication date range starts at {from} after it ends at {to}"
        )));
    }
    if let Some(organization) = &filters.organization
        && organization.names.iter().any(|name| name.trim().is_empty())
    {
        return Err(SearchError::InvalidInput(
            "organization names must not be empty".into(),
        ));
    }
    Ok(())
}

pub fn filter_query(filters: &LiteratureFilters) -> String {
    let mut patterns = vec!["$document isa document, has pdf_hash $pdf_hash".to_owned()];
    if !filters.document_types.is_empty() {
        patterns.push(or_patterns(
            filters
                .document_types
                .iter()
                .map(|kind| format!("$document isa! {}", kind.label())),
        ));
    }
    if let Some(range) = &filters.publication_date {
        patterns.push(
            "$publication isa publication, links (work: $document), has publication_date $date"
                .into(),
        );
        if let Some(from) = range.from {
            patterns.push(format!("$date >= {}T00:00:00", from.format("%Y-%m-%d")));
        }
        if let Some(to) = range.to {
            match to.succ_opt() {
                Some(exclusive) => {
                    patterns.push(format!("$date < {}T00:00:00", exclusive.format("%Y-%m-%d")))
                }
                None => patterns.push(format!("$date <= {}T23:59:59", to.format("%Y-%m-%d"))),
            }
        }
    }
    if let Some(organization) = &filters.organization {
        let roles = if organization.roles.is_empty()
            || organization.roles.contains(&OrganizationRoleFilter::Any)
        {
            vec![
                OrganizationRoleFilter::Publisher,
                OrganizationRoleFilter::Affiliation,
                OrganizationRoleFilter::Contributor,
            ]
        } else {
            organization.roles.clone()
        };
        patterns.push(or_patterns(roles.into_iter().map(|role| match role {
            OrganizationRoleFilter::Publisher => "$relation isa publication_event, links (work: $document, publisher: $organization)".into(),
            OrganizationRoleFilter::Affiliation => "$relation isa affiliation, links (evidence: $document, organization: $organization)".into(),
            OrganizationRoleFilter::Contributor => "$relation isa contribution, links (work: $document, contributor: $organization)".into(),
            OrganizationRoleFilter::Any => unreachable!(),
        })));
        if organization.types.is_empty() {
            patterns.push("$organization isa organization".into());
        } else {
            patterns.push(or_patterns(
                organization
                    .types
                    .iter()
                    .map(|kind| format!("$organization isa {}", kind.label())),
            ));
        }
        if !organization.names.is_empty() {
            patterns.push("$organization has organization_name $organization_name".into());
            patterns.push(or_patterns(organization.names.iter().map(|name| {
                format!(
                    "$organization_name contains {}",
                    serde_json::to_string(name.trim()).expect("strings always serialize")
                )
            })));
        }
    }
    format!("match {}; select $pdf_hash;", patterns.join("; "))
}

fn or_patterns(patterns: impl IntoIterator<Item = String>) -> String {
    patterns
        .into_iter()
        .map(|pattern| format!("{{ {pattern}; }}"))
        .collect::<Vec<_>>()
        .join(" or ")
}

fn row_concept<'a>(row: &'a ConceptRow, name: &str) -> Result<&'a Concept, SearchError> {
    row.get(name)
        .map_err(typedb_error)?
        .ok_or_else(|| SearchError::TypeDb(format!("query returned empty `${name}`")))
}

fn row_string(row: &ConceptRow, name: &str) -> Result<String, SearchError> {
    row_concept(row, name)?
        .try_get_string()
        .map(str::to_owned)
        .ok_or_else(|| SearchError::TypeDb(format!("`${name}` is not a string")))
}

fn row_label(row: &ConceptRow, name: &str) -> Result<String, SearchError> {
    row_concept(row, name)?
        .try_get_label()
        .map(str::to_owned)
        .ok_or_else(|| SearchError::TypeDb(format!("`${name}` has no type label")))
}

fn row_datetime(row: &ConceptRow, name: &str) -> Result<String, SearchError> {
    row_concept(row, name)?
        .try_get_datetime()
        .map(|value| value.format("%Y-%m-%dT%H:%M:%S").to_string())
        .ok_or_else(|| SearchError::TypeDb(format!("`${name}` is not a datetime")))
}

fn find_event<'a>(
    documents: &'a mut BTreeMap<String, DocumentMetadata>,
    hash: &str,
    event_type: &str,
    date: &str,
) -> Option<&'a mut PublicationEventMetadata> {
    documents
        .get_mut(hash)?
        .publication_events
        .iter_mut()
        .find(|event| event.event_type == event_type && event.publication_date == date)
}

fn typedb_error(error: impl std::fmt::Display) -> SearchError {
    SearchError::TypeDb(error.to_string())
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::models::{
        DocumentTypeFilter, OrganizationFilter, OrganizationTypeFilter, PublicationDateFilter,
    };

    #[test]
    fn query_combines_allowlisted_filters_and_escapes_names() {
        let filters = LiteratureFilters {
            publication_date: Some(PublicationDateFilter {
                from: Some(NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()),
                to: Some(NaiveDate::from_ymd_opt(2024, 12, 31).unwrap()),
            }),
            document_types: vec![DocumentTypeFilter::ResearchPaper],
            organization: Some(OrganizationFilter {
                names: vec!["ACME\"; delete $x".into()],
                roles: vec![OrganizationRoleFilter::Affiliation],
                types: vec![OrganizationTypeFilter::Institution],
            }),
        };
        let query = filter_query(&filters);
        assert!(query.contains("isa! research_paper"));
        assert!(query.contains("isa institution"));
        assert!(query.contains("2025-01-01T00:00:00"));
        assert!(query.contains(r#"contains "ACME\"; delete $x""#));
    }
}
