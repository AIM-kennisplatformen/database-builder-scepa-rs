use serde::{Deserialize, Serialize};

/// Prose and formulae retain their distinction in the serialized output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Passage {
    Text(TextPassage),
    Formula(FormulaPassage),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema)]
#[builder(on(String, into))]
pub struct TextPassage {
    #[serde(default)]
    pub id: String,
    pub text: String,
    pub coordinates: Vec<BoundingBox>,
    pub heading_context: Option<String>,
    pub section: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema)]
#[builder(on(String, into))]
pub struct FormulaPassage {
    #[serde(default)]
    pub id: String,
    pub text: String,
    pub label: Option<String>,
    pub coordinates: Vec<BoundingBox>,
    pub heading_context: Option<String>,
    pub section: Option<String>,
}

/// Coordinates use Grobid's `page,x,y,width,height` convention.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema)]
pub struct BoundingBox {
    pub page: Option<u32>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
