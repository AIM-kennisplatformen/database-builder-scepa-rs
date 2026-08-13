//! Document-level bibliographic metadata and abstract extraction.

use crate::models::draft::{
    Bibliography, ContributorRole, IdentifierScope, PassageLevel, TextPassage,
};

use super::{
    body::parse_text_passage,
    common::{extract_year, non_empty_text, parse_contributor, parse_identifier},
    xml::XmlElement,
};

pub(super) fn parse_bibliography(header: &XmlElement, level: PassageLevel) -> Bibliography {
    let title = header
        .descendants_named("title")
        .into_iter()
        .find(|element| element.attr("type") == Some("main") && element.attr("level") == Some("a"))
        .and_then(non_empty_text);

    let authors = header
        .descendants_named("author")
        .into_iter()
        .filter_map(|author| parse_contributor(author, ContributorRole::Author))
        .collect();

    let identifiers = header
        .descendants_named("idno")
        .into_iter()
        .filter_map(|identifier| parse_identifier(identifier, IdentifierScope::Document))
        .collect();

    let publication_date = header
        .descendants_named("date")
        .into_iter()
        .find(|date| date.attr("type") == Some("published"))
        .and_then(|date| {
            date.attr("when")
                .map(str::to_owned)
                .or_else(|| non_empty_text(date))
        });
    let publication_year = publication_date.as_deref().and_then(extract_year);

    let publisher = header
        .descendants_named("publicationStmt")
        .first()
        .and_then(|statement| {
            statement
                .descendants_named("publisher")
                .first()
                .and_then(|publisher| non_empty_text(publisher))
        });
    let journal = header
        .descendants_named("title")
        .into_iter()
        .find(|title| title.attr("level") == Some("j") && title.attr("type") != Some("abbr"))
        .and_then(non_empty_text);
    let journal_abbreviation = find_typed_title(header, "abbr", "j");
    let abstract_text = header
        .descendants_named("abstract")
        .first()
        .map(|abstract_element| parse_abstract(abstract_element, level))
        .unwrap_or_default();

    Bibliography {
        title,
        authors,
        identifiers,
        publication_date,
        publication_year,
        publisher,
        journal,
        journal_abbreviation,
        abstract_text,
    }
}

fn find_typed_title(element: &XmlElement, kind: &str, level: &str) -> Option<String> {
    element
        .descendants_named("title")
        .into_iter()
        .find(|title| title.attr("type") == Some(kind) && title.attr("level") == Some(level))
        .and_then(non_empty_text)
}

fn parse_abstract(element: &XmlElement, level: PassageLevel) -> Vec<TextPassage> {
    let mut passages = Vec::new();
    for (paragraph_index, paragraph) in element.descendants_named("p").into_iter().enumerate() {
        match level {
            PassageLevel::Sentence => {
                for (sentence_index, sentence) in
                    paragraph.descendants_named("s").into_iter().enumerate()
                {
                    let fallback =
                        format!("abstract_{}_{}", paragraph_index + 1, sentence_index + 1);
                    passages.push(parse_text_passage(sentence, fallback, None, None));
                }
            }
            PassageLevel::Paragraph => {
                let fallback = format!("abstract_{}", paragraph_index + 1);
                passages.push(parse_text_passage(paragraph, fallback, None, None));
            }
        }
    }
    passages
}
