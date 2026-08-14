//! Root canonical model and conversion from the extraction draft.

use super::entities::document::{Book, Document, ResearchPaper};
use super::entities::person::Person;
use crate::models::draft::{ContributorRole, Identifier, IdentifierKind, TeiDocument};

/// The complete canonical value accepted by persistence services.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalModel {
    pub document: CanonicalDocument,
    pub contributors: Vec<CanonicalContributor>,
}

/// A canonical person and their contribution to the model's document.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalContributor {
    pub person: Person,
    pub contribution: CanonicalContribution,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalContribution {
    Authorship,
    Contribution,
}

/// A document in the canonical domain model.
///
/// The variant determines the TypeDB entity type used when the model is
/// persisted. More specialised variants retain their identifying attribute.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CanonicalDocument {
    Document(Document),
    ResearchPaper(ResearchPaper),
    Book(Book),
}

impl CanonicalDocument {
    pub fn document_id(&self) -> &str {
        match self {
            Self::Document(document) => &document.document_id,
            Self::ResearchPaper(document) => &document.document_id,
            Self::Book(document) => &document.document_id,
        }
    }

    pub fn pdf_hash(&self) -> Option<&str> {
        match self {
            Self::Document(document) => document.pdf_hash.as_deref(),
            Self::ResearchPaper(document) => document.pdf_hash.as_deref(),
            Self::Book(document) => document.pdf_hash.as_deref(),
        }
    }

    pub fn set_pdf_hash(&mut self, pdf_hash: String) {
        match self {
            Self::Document(document) => document.pdf_hash = Some(pdf_hash),
            Self::ResearchPaper(document) => document.pdf_hash = Some(pdf_hash),
            Self::Book(document) => document.pdf_hash = Some(pdf_hash),
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::Document(document) => &document.title,
            Self::ResearchPaper(document) => &document.title,
            Self::Book(document) => &document.title,
        }
    }

    pub fn entity_type(&self) -> &'static str {
        match self {
            Self::Document(_) => "document",
            Self::ResearchPaper(_) => "research_paper",
            Self::Book(_) => "book",
        }
    }

    fn try_from_draft(
        draft: &TeiDocument,
        fallback_document_id: Option<String>,
    ) -> eros::Result<Self> {
        let Some(title) = draft
            .bibliography
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
        else {
            eros::bail!("canonical document requires a title")
        };
        let title = title.to_owned();

        let doi = identifier(&draft.bibliography.identifiers, |kind| {
            matches!(kind, IdentifierKind::Doi)
        });
        let isbn = identifier(&draft.bibliography.identifiers, |kind| {
            matches!(kind, IdentifierKind::Isbn)
        });
        if let Some(doi) = doi {
            return Ok(Self::ResearchPaper(ResearchPaper {
                document_id: doi.to_owned(),
                pdf_hash: None,
                title,
                doi: Some(doi.to_owned()),
            }));
        }

        if let Some(isbn) = isbn {
            return Ok(Self::Book(Book {
                document_id: isbn.to_owned(),
                pdf_hash: None,
                title,
                isbn: Some(isbn.to_owned()),
            }));
        }

        let Some(document_id) = draft
            .bibliography
            .identifiers
            .iter()
            .filter(|identifier| {
                !matches!(identifier.kind, IdentifierKind::Issn | IdentifierKind::Md5)
            })
            .map(|identifier| identifier.value.trim())
            .find(|value| !value.is_empty())
            .map(str::to_owned)
            .or(fallback_document_id)
        else {
            eros::bail!("canonical document requires a stable document identifier")
        };

        Ok(Self::Document(Document {
            document_id,
            pdf_hash: None,
            title,
        }))
    }
}

impl TryFrom<&TeiDocument> for CanonicalDocument {
    type Error = eros::ErrorUnion;

    fn try_from(draft: &TeiDocument) -> Result<Self, Self::Error> {
        Self::try_from_draft(draft, None)
    }
}

impl CanonicalModel {
    /// Canonicalises a draft, using the exact PDF's SHA-256 as its identifier
    /// only when the draft has no usable bibliographic identifier.
    pub fn try_from_with_pdf_hash(draft: &TeiDocument, pdf_hash: &str) -> eros::Result<Self> {
        if pdf_hash.len() != 64
            || !pdf_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            eros::bail!("PDF hash must be a lowercase SHA-256 digest")
        }

        let fallback_document_id = format!("sha256:{pdf_hash}");
        let mut document = CanonicalDocument::try_from_draft(draft, Some(fallback_document_id))?;
        document.set_pdf_hash(pdf_hash.to_owned());
        Self::from_document_and_draft(document, draft)
    }

    fn from_document_and_draft(
        document: CanonicalDocument,
        draft: &TeiDocument,
    ) -> eros::Result<Self> {
        if draft.bibliography.authors.is_empty() {
            eros::bail!("canonical document requires at least one contributor")
        }

        let contributors = draft
            .bibliography
            .authors
            .iter()
            .enumerate()
            .map(|(index, contributor)| {
                let given_name = contributor
                    .forename
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned);
                let family_name = contributor
                    .surname
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .or_else(|| {
                        (given_name.is_none())
                            .then(|| contributor.name.trim())
                            .filter(|name| !name.is_empty())
                    })
                    .map(str::to_owned);

                if given_name.is_none() && family_name.is_none() {
                    eros::bail!("canonical contributor {} requires a name", index + 1)
                }

                Ok(CanonicalContributor {
                    person: Person {
                        person_id: format!("{}:contributor:{}", document.document_id(), index + 1),
                        given_name,
                        family_name,
                    },
                    contribution: match contributor.role {
                        ContributorRole::Author => CanonicalContribution::Authorship,
                        ContributorRole::Editor => CanonicalContribution::Contribution,
                    },
                })
            })
            .collect::<eros::Result<Vec<_>>>()?;

        Ok(Self {
            document,
            contributors,
        })
    }
}

impl TryFrom<&TeiDocument> for CanonicalModel {
    type Error = eros::ErrorUnion;

    fn try_from(draft: &TeiDocument) -> Result<Self, Self::Error> {
        let document = CanonicalDocument::try_from(draft)?;
        Self::from_document_and_draft(document, draft)
    }
}

fn identifier(
    identifiers: &[Identifier],
    matches_kind: impl Fn(&IdentifierKind) -> bool,
) -> Option<&str> {
    identifiers
        .iter()
        .find(|identifier| matches_kind(&identifier.kind) && !identifier.value.trim().is_empty())
        .map(|identifier| identifier.value.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::draft::{Bibliography, Contributor, IdentifierScope, PassageLevel};

    fn draft(title: Option<&str>, identifiers: Vec<Identifier>) -> TeiDocument {
        TeiDocument {
            level: PassageLevel::Paragraph,
            bibliography: Bibliography {
                title: title.map(str::to_owned),
                identifiers,
                authors: vec![Contributor {
                    name: "Ada Lovelace".to_owned(),
                    forename: Some("Ada".to_owned()),
                    surname: Some("Lovelace".to_owned()),
                    affiliation: None,
                    role: ContributorRole::Author,
                }],
                ..Bibliography::default()
            },
            body_text: vec![],
            figures_and_tables: vec![],
            references: vec![],
        }
    }

    fn id(kind: IdentifierKind, value: &str) -> Identifier {
        Identifier {
            kind,
            value: value.to_owned(),
            scope: IdentifierScope::Document,
        }
    }

    #[test]
    fn doi_draft_becomes_a_canonical_research_paper() {
        let canonical = CanonicalDocument::try_from(&draft(
            Some(" A paper "),
            vec![id(IdentifierKind::Doi, "10.1234/example")],
        ))
        .unwrap();

        assert_eq!(canonical.document_id(), "10.1234/example");
        assert_eq!(canonical.title(), "A paper");
        assert!(matches!(canonical, CanonicalDocument::ResearchPaper(_)));
    }

    #[test]
    fn draft_without_required_canonical_fields_is_rejected() {
        assert!(CanonicalDocument::try_from(&draft(None, vec![])).is_err());
        assert!(CanonicalDocument::try_from(&draft(Some("A paper"), vec![])).is_err());
    }

    #[test]
    fn canonical_model_contains_required_contribution() {
        let canonical = CanonicalModel::try_from(&draft(
            Some("A paper"),
            vec![id(IdentifierKind::Doi, "10.1234/example")],
        ))
        .unwrap();

        assert_eq!(canonical.contributors.len(), 1);
        assert_eq!(
            canonical.contributors[0].contribution,
            CanonicalContribution::Authorship
        );
    }
}
