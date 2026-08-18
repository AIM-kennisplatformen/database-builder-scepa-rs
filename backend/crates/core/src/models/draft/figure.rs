use serde::{Deserialize, Serialize};

use crate::models::draft::passage::BoundingBox;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FigureOrTable {
    Figure(Figure),
    Table(Table),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema)]
#[builder(on(String, into))]
pub struct Figure {
    pub id: String,
    pub label: Option<String>,
    pub heading: Option<String>,
    pub description: Option<String>,
    pub note: Option<String>,
    pub coordinates: Vec<BoundingBox>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema)]
#[builder(on(String, into))]
pub struct Table {
    pub id: String,
    pub label: Option<String>,
    pub heading: Option<String>,
    pub description: Option<String>,
    pub note: Option<String>,
    pub coordinates: Vec<BoundingBox>,
    pub content: Option<TableContent>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bon::Builder, utoipa::ToSchema)]
pub struct TableContent {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub row_count: usize,
    pub column_count: usize,
}
