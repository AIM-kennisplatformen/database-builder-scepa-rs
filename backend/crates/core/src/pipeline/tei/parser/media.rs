//! Figure and table extraction.

use crate::models::draft::{Figure, FigureOrTable, Table, TableContent};

use super::{
    Counters,
    common::{non_empty_text, parse_coordinates},
    xml::XmlElement,
};

pub(super) fn parse_figures(text: &XmlElement, counters: &mut Counters) -> Vec<FigureOrTable> {
    text.descendants_named("figure")
        .into_iter()
        .map(|element| {
            counters.media += 1;
            let id = element
                .attr("id")
                .map(str::to_owned)
                .unwrap_or_else(|| format!("media_{:08}", counters.media));
            let label = element.child("label").and_then(non_empty_text);
            let heading = element.child("head").and_then(non_empty_text);
            let description = element.child("figDesc").and_then(non_empty_text);
            let note = element.child("note").and_then(non_empty_text);
            if element.attr("type") == Some("table") {
                FigureOrTable::Table(Table {
                    id,
                    label,
                    heading,
                    description,
                    note,
                    coordinates: parse_coordinates(element.attr("coords")),
                    content: element.child("table").and_then(parse_table),
                })
            } else {
                let coordinates = element
                    .child("graphic")
                    .and_then(|graphic| graphic.attr("coords"))
                    .map(|coords| parse_coordinates(Some(coords)))
                    .unwrap_or_else(|| parse_coordinates(element.attr("coords")));
                FigureOrTable::Figure(Figure {
                    id,
                    label,
                    heading,
                    description,
                    note,
                    coordinates,
                })
            }
        })
        .collect()
}

fn parse_table(table: &XmlElement) -> Option<TableContent> {
    let header_rows = table
        .child("thead")
        .map(|thead| thead.descendants_named("row"))
        .unwrap_or_default();
    let headers: Vec<String> = header_rows
        .first()
        .map(|row| {
            row.descendants_named("cell")
                .into_iter()
                .map(XmlElement::text)
                .collect()
        })
        .unwrap_or_default();

    let rows_source = table
        .child("tbody")
        .map(|tbody| tbody.descendants_named("row"))
        .unwrap_or_else(|| {
            table
                .descendants_named("row")
                .into_iter()
                .skip(header_rows.len())
                .collect()
        });
    let rows: Vec<Vec<String>> = rows_source
        .into_iter()
        .map(|row| {
            row.descendants_named("cell")
                .into_iter()
                .map(XmlElement::text)
                .collect()
        })
        .filter(|row: &Vec<String>| !row.is_empty())
        .collect();
    if rows.is_empty() {
        return None;
    }
    let column_count = headers
        .len()
        .max(rows.iter().map(Vec::len).max().unwrap_or(0));
    Some(TableContent {
        row_count: rows.len(),
        column_count,
        headers,
        rows,
    })
}
