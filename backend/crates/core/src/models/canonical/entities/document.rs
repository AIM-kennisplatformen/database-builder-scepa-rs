use enum_dispatch::enum_dispatch;

use crate::models::draft::{DocumentClassification, LiteratureKind};

#[enum_dispatch]
pub trait TDocument: Send + Sync {
    fn document_id(&self) -> &str;
    fn pdf_hash(&self) -> Option<&str>;
    fn title(&self) -> &str;
    fn description(&self) -> Option<&str>;
    fn classification(&self) -> &DocumentClassification;
    fn entity_type(&self) -> &'static str;
    fn doi(&self) -> Option<&str>;
    fn isbn(&self) -> Option<&str>;
}

fn entity_type(classification: &DocumentClassification) -> &'static str {
    match classification.literature_kind {
        None => "document",
        Some(LiteratureKind::GreyLiterature) => "grey_literature",
        Some(LiteratureKind::ScientificLiterature) => "scientific_literature",
        Some(LiteratureKind::ProjectReport) => "project_report",
    }
}

macro_rules! document_type {
    ($name:ident) => {
        #[derive(
            Clone,
            Debug,
            PartialEq,
            serde::Serialize,
            serde::Deserialize,
            bon::Builder,
            utoipa::ToSchema,
        )]
        #[builder(on(String, into))]
        pub struct $name {
            pub document_id: String,
            pub pdf_hash: Option<String>,
            pub title: String,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub description: Option<String>,
            #[serde(default)]
            pub classification: DocumentClassification,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub doi: Option<String>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub isbn: Option<String>,
        }

        impl TDocument for $name {
            fn document_id(&self) -> &str {
                &self.document_id
            }

            fn pdf_hash(&self) -> Option<&str> {
                self.pdf_hash.as_deref()
            }

            fn title(&self) -> &str {
                &self.title
            }

            fn description(&self) -> Option<&str> {
                self.description.as_deref()
            }

            fn classification(&self) -> &DocumentClassification {
                &self.classification
            }

            fn entity_type(&self) -> &'static str {
                entity_type(&self.classification)
            }

            fn doi(&self) -> Option<&str> {
                self.doi.as_deref()
            }

            fn isbn(&self) -> Option<&str> {
                self.isbn.as_deref()
            }
        }
    };
}

document_type!(Document);
document_type!(GreyLiterature);
document_type!(ScientificLiterature);
document_type!(ProjectReport);

#[enum_dispatch(TDocument)]
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EDocument {
    Document,
    GreyLiterature,
    ScientificLiterature,
    ProjectReport,
}
