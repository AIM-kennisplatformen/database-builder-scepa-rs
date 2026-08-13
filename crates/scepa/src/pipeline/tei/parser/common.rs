//! Shared extraction helpers for values used across TEI sections.

use crate::models::draft::{
    BoundingBox, Contributor, ContributorRole, Identifier, IdentifierKind, IdentifierScope,
};

use super::xml::XmlElement;

pub(super) fn parse_contributor(
    element: &XmlElement,
    role: ContributorRole,
) -> Option<Contributor> {
    let forename = element
        .descendants_named("forename")
        .first()
        .and_then(|name| non_empty_text(name));
    let surname = element
        .descendants_named("surname")
        .first()
        .and_then(|name| non_empty_text(name));
    let name = match (&forename, &surname) {
        (Some(forename), Some(surname)) => format!("{forename} {surname}"),
        (Some(forename), None) => forename.clone(),
        (None, Some(surname)) => surname.clone(),
        (None, None) => element
            .child("persName")
            .and_then(non_empty_text)
            .or_else(|| non_empty_text(element))?,
    };
    let affiliation = element
        .descendants_named("affiliation")
        .first()
        .and_then(|affiliation| non_empty_text(affiliation));
    Some(Contributor {
        name,
        forename,
        surname,
        affiliation,
        role,
    })
}

pub(super) fn parse_identifier(element: &XmlElement, scope: IdentifierScope) -> Option<Identifier> {
    let value = non_empty_text(element)?;
    let raw_kind = element.attr("type").unwrap_or("unknown");
    let kind = match raw_kind.to_ascii_lowercase().as_str() {
        "doi" => IdentifierKind::Doi,
        "isbn" => IdentifierKind::Isbn,
        "issn" => IdentifierKind::Issn,
        "pmc" => IdentifierKind::Pmc,
        "pmid" => IdentifierKind::Pmid,
        "arxiv" => IdentifierKind::Arxiv,
        "md5" => IdentifierKind::Md5,
        _ => IdentifierKind::Other(raw_kind.to_owned()),
    };
    Some(Identifier { kind, value, scope })
}

pub(super) fn parse_coordinates(value: Option<&str>) -> Vec<BoundingBox> {
    value
        .into_iter()
        .flat_map(|value| value.split(';'))
        .filter_map(|coordinate| {
            let values: Vec<_> = coordinate
                .split(',')
                .map(str::trim)
                .filter_map(|value| value.parse::<f64>().ok())
                .collect();
            match values.as_slice() {
                [x, y, width, height] => Some(BoundingBox {
                    page: None,
                    x: *x,
                    y: *y,
                    width: *width,
                    height: *height,
                }),
                [page, x, y, width, height] => Some(BoundingBox {
                    page: (*page >= 0.0).then_some(*page as u32),
                    x: *x,
                    y: *y,
                    width: *width,
                    height: *height,
                }),
                _ => None,
            }
        })
        .collect()
}

pub(super) fn non_empty_text(element: &XmlElement) -> Option<String> {
    let text = element.text();
    (!text.is_empty()).then_some(text)
}

pub(super) fn extract_year(value: &str) -> Option<u16> {
    value
        .as_bytes()
        .windows(4)
        .filter_map(|digits| std::str::from_utf8(digits).ok())
        .filter_map(|digits| digits.parse::<u16>().ok())
        .find(|year| (1900..=2099).contains(year))
}
