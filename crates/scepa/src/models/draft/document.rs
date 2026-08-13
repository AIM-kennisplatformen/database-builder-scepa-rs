use serde::{Deserialize, Serialize};

use crate::models::draft::{
    bibliography::Bibliography, citation::Citation, figure::FigureOrTable, passage::Passage,
};

/// Whether prose was segmented into paragraphs or sentences by the TEI producer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PassageLevel {
    Paragraph,
    Sentence,
}

/// A complete application-facing representation of a TEI document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder)]
pub struct TeiDocument {
    pub level: PassageLevel,
    pub bibliography: Bibliography,
    pub body_text: Vec<Passage>,
    pub figures_and_tables: Vec<FigureOrTable>,
    pub references: Vec<Citation>,
}
