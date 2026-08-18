use serde::{Deserialize, Serialize};

use crate::models::draft::{
    bibliography::{Bibliography, Contributor, Identifier},
    citation::Citation,
    figure::FigureOrTable,
    passage::Passage,
};

/// Whether prose was segmented into paragraphs or sentences by the TEI producer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PassageLevel {
    Paragraph,
    Sentence,
}

/// A complete application-facing representation of a TEI document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema)]
pub struct TeiDocument {
    pub level: PassageLevel,
    pub bibliography: Bibliography,
    pub body_text: Vec<Passage>,
    pub figures_and_tables: Vec<FigureOrTable>,
    pub references: Vec<Citation>,
}

/// An extraction draft together with sparse, operator-authored overrides.
///
/// The serialized field names intentionally describe the persisted artifact,
/// while the Rust names describe how the values are used in the application.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DraftDocument {
    #[serde(rename = "grobid_extraction_data")]
    pub extracted_data: TeiDocument,
    pub manual_data: ManualDocument,
}

impl DraftDocument {
    pub fn new(extracted_data: TeiDocument) -> Self {
        Self {
            extracted_data,
            manual_data: ManualDocument::default(),
        }
    }

    /// Produces the TEI-shaped input used for canonicalisation.
    ///
    /// Extraction evidence is retained in `self`; only this derived value is
    /// overlaid. A populated manual field always wins, including an empty list.
    pub fn effective_document(&self) -> TeiDocument {
        let mut effective = self.extracted_data.clone();
        let manual = &self.manual_data.bibliography;

        if let Some(value) = &manual.title {
            effective.bibliography.title = Some(value.clone());
        }
        if let Some(value) = &manual.authors {
            effective.bibliography.authors = value.clone();
        }
        if let Some(value) = &manual.identifiers {
            effective.bibliography.identifiers = value.clone();
        }
        if let Some(value) = &manual.publication_date {
            effective.bibliography.publication_date = Some(value.clone());
        }
        if let Some(value) = manual.publication_year {
            effective.bibliography.publication_year = Some(value);
        }
        if let Some(value) = &manual.publisher {
            effective.bibliography.publisher = Some(value.clone());
        }
        if let Some(value) = &manual.journal {
            effective.bibliography.journal = Some(value.clone());
        }
        if let Some(value) = &manual.journal_abbreviation {
            effective.bibliography.journal_abbreviation = Some(value.clone());
        }
        if let Some(value) = &manual.abstract_text {
            effective.bibliography.abstract_text = value.clone();
        }
        if let Some(value) = &self.manual_data.body_text {
            effective.body_text = value.clone();
        }

        effective
    }
}

/// Human-authored values that may override extraction for canonicalisation.
///
/// Besides metadata, operators can correct the abstract/body classification and
/// the text or source coordinates of passages. The extracted layer remains
/// immutable; populated passage fields replace it only in the effective view.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default)]
pub struct ManualDocument {
    pub bibliography: ManualBibliography,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_text: Option<Vec<Passage>>,
}

/// Sparse bibliography patch. `None` means “use the extracted value”; for
/// collections, `Some(vec![])` intentionally replaces extraction with no rows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default)]
pub struct ManualBibliography {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<Contributor>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifiers: Option<Vec<Identifier>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication_year: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journal_abbreviation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstract_text: Option<Vec<crate::models::draft::passage::TextPassage>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extracted() -> TeiDocument {
        TeiDocument {
            level: PassageLevel::Paragraph,
            bibliography: Bibliography {
                title: Some("Extracted title".into()),
                ..Bibliography::default()
            },
            body_text: vec![],
            figures_and_tables: vec![],
            references: vec![],
        }
    }

    #[test]
    fn artifact_uses_storage_contract_field_names() {
        let value = serde_json::to_value(DraftDocument::new(extracted())).unwrap();
        assert!(value.get("grobid_extraction_data").is_some());
        assert_eq!(
            value["manual_data"],
            serde_json::json!({ "bibliography": {} })
        );
        assert!(value.get("extracted_data").is_none());
    }

    #[test]
    fn manual_values_overlay_without_mutating_extraction() {
        let mut draft = DraftDocument::new(extracted());
        draft.manual_data.bibliography.title = Some("Reviewed title".into());
        draft.manual_data.bibliography.authors = Some(vec![]);
        draft.manual_data.body_text = Some(vec![]);
        draft.manual_data.bibliography.abstract_text = Some(vec![]);

        let effective = draft.effective_document();
        assert_eq!(
            effective.bibliography.title.as_deref(),
            Some("Reviewed title")
        );
        assert!(effective.bibliography.authors.is_empty());
        assert!(effective.body_text.is_empty());
        assert!(effective.bibliography.abstract_text.is_empty());
        assert_eq!(
            draft.extracted_data.bibliography.title.as_deref(),
            Some("Extracted title")
        );
    }
}
