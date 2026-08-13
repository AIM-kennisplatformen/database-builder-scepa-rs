//! Bibliography-entry extraction from TEI `listBibl` structures.

use crate::models::draft::{
    Citation, CitationNote, ContributorRole, IdentifierScope, PublicationMetadata,
};

use super::{
    common::{extract_year, non_empty_text, parse_contributor, parse_identifier},
    xml::XmlElement,
};

pub(super) fn parse_citation(element: &XmlElement, index: usize) -> Option<Citation> {
    let analytic = element.child("analytic");
    let monograph = element.child("monogr");
    let series = element.child("series");

    let title = analytic
        .and_then(|part| title_at_level(part, "a"))
        .or_else(|| monograph.and_then(|part| title_at_level(part, "m")));
    let mut publication = PublicationMetadata {
        journal: analytic
            .and_then(|part| title_at_level(part, "j"))
            .or_else(|| monograph.and_then(|part| title_at_level(part, "j"))),
        series: monograph
            .and_then(|part| title_at_level(part, "s"))
            .or_else(|| {
                series.and_then(|part| {
                    part.descendants_named("title")
                        .first()
                        .and_then(|title| non_empty_text(title))
                })
            }),
        ..PublicationMetadata::default()
    };
    if let Some(imprint) = monograph.and_then(|part| part.child("imprint")) {
        parse_imprint(imprint, &mut publication);
    }

    let mut contributors = Vec::new();
    for part in [analytic, monograph, series].into_iter().flatten() {
        for author in part.children_named("author") {
            if let Some(author) = parse_contributor(author, ContributorRole::Author) {
                contributors.push(author);
            }
        }
        for editor in part.children_named("editor") {
            if let Some(editor) = parse_contributor(editor, ContributorRole::Editor) {
                contributors.push(editor);
            }
        }
    }

    let mut identifiers = Vec::new();
    if let Some(part) = analytic {
        identifiers.extend(
            part.children_named("idno")
                .filter_map(|id| parse_identifier(id, IdentifierScope::Analytic)),
        );
    }
    if let Some(part) = monograph {
        identifiers.extend(
            part.children_named("idno")
                .filter_map(|id| parse_identifier(id, IdentifierScope::Monograph)),
        );
    }
    identifiers.extend(
        element
            .children_named("idno")
            .filter_map(|id| parse_identifier(id, IdentifierScope::Citation)),
    );

    let analytic_reference =
        analytic.and_then(|part| part.descendants_named("ref").first().copied());
    let reference_text = analytic_reference.and_then(non_empty_text);
    let mut urls = Vec::new();
    if let Some(target) = analytic_reference.and_then(|reference| reference.attr("target")) {
        urls.push(target.to_owned());
    }
    urls.extend(
        element
            .descendants_named("ptr")
            .into_iter()
            .filter_map(|pointer| pointer.attr("target").map(str::to_owned)),
    );
    urls.sort();
    urls.dedup();

    let mut raw_reference = None;
    let mut notes = Vec::new();
    for note in element.descendants_named("note") {
        let Some(text) = non_empty_text(note) else {
            continue;
        };
        if note.attr("type") == Some("raw_reference") {
            raw_reference = Some(text);
        } else {
            notes.push(CitationNote {
                kind: note.attr("type").map(str::to_owned),
                text,
            });
        }
    }

    let meaningful = title.is_some()
        || !contributors.is_empty()
        || publication.journal.is_some()
        || !identifiers.is_empty()
        || reference_text.is_some()
        || raw_reference.is_some();
    meaningful.then(|| Citation {
        id: format!("b{index}"),
        target: element.attr("id").map(str::to_owned),
        title,
        contributors,
        publication,
        identifiers,
        reference_text,
        raw_reference,
        notes,
        urls,
    })
}

fn parse_imprint(imprint: &XmlElement, publication: &mut PublicationMetadata) {
    if let Some(publisher) = imprint.descendants_named("publisher").first() {
        publication.publisher = non_empty_text(publisher);
        publication.publisher_location = publisher.attr("from").map(str::to_owned);
    }
    for date in imprint.descendants_named("date") {
        let value = date
            .attr("when")
            .map(str::to_owned)
            .or_else(|| non_empty_text(date));
        if let Some(value) = value {
            publication.year = extract_year(&value).or(publication.year);
            publication.publication_date = Some(value);
        }
    }
    for scope in imprint.descendants_named("biblScope") {
        let text = non_empty_text(scope);
        match scope.attr("unit").unwrap_or_default() {
            "page" => {
                publication.page_start = scope.attr("from").map(str::to_owned);
                publication.page_end = scope.attr("to").map(str::to_owned);
                if publication.page_start.is_none() && publication.page_end.is_none() {
                    publication.pages = text;
                }
            }
            "volume" | "vol" => publication.volume = text,
            "issue" | "num" => publication.issue = text,
            "chapter" => publication.chapter = text,
            _ => {}
        }
    }
}

fn title_at_level(element: &XmlElement, level: &str) -> Option<String> {
    element
        .descendants_named("title")
        .into_iter()
        .find(|title| title.attr("level") == Some(level))
        .and_then(non_empty_text)
}
