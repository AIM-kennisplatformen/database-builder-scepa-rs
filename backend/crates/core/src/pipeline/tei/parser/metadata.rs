//! Document-level bibliographic metadata and abstract extraction.

use crate::models::draft::{
    Bibliography, ContributorRole, DraftOrganization, DraftPublicationVenue, IdentifierKind,
    IdentifierScope, PassageLevel, TextPassage,
};
use chrono::NaiveDate;

use super::{
    body::parse_text_passage,
    common::{non_empty_text, parse_contributor, parse_identifier},
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

    let identifiers: Vec<_> = header
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
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
                .or_else(|| non_empty_text(date))
        })
        .and_then(|value| normalize_publication_date(&value));

    let publisher = header
        .descendants_named("publicationStmt")
        .first()
        .and_then(|statement| {
            statement
                .descendants_named("publisher")
                .first()
                .and_then(|publisher| non_empty_text(publisher))
        })
        .map(|name| DraftOrganization {
            id: String::new(),
            name,
            ror_id: None,
        });
    let journal_name = header
        .descendants_named("title")
        .into_iter()
        .find(|title| title.attr("level") == Some("j") && title.attr("type") != Some("abbr"))
        .and_then(non_empty_text);
    let journal_abbreviation = find_typed_title(header, "abbr", "j");
    let journal = journal_name.map(|name| DraftPublicationVenue {
        id: String::new(),
        name,
        abbreviation: journal_abbreviation.clone(),
        issn: identifiers
            .iter()
            .find(|identifier| matches!(identifier.kind, IdentifierKind::Issn))
            .map(|identifier| identifier.value.clone()),
    });
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
        publisher,
        journal,
        journal_abbreviation,
        publication_event_id: None,
        abstract_text,
    }
}

fn normalize_publication_date(value: &str) -> Option<String> {
    let value = value.trim();
    if let Some(date) = value.get(..10)
        && matches!(value.as_bytes().get(10), Some(b'T' | b' '))
        && let Ok(date) = NaiveDate::parse_from_str(date, "%Y-%m-%d")
    {
        return Some(date.to_string());
    }
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return Some(date.to_string());
    }
    if let Ok(date) = NaiveDate::parse_from_str(&format!("{value}-01"), "%Y-%m-%d") {
        return Some(date.to_string());
    }
    if let Ok(year) = value.parse::<i32>() {
        return NaiveDate::from_ymd_opt(year, 1, 1).map(|date| date.to_string());
    }
    // Preserve an extractable year when the full TEI date is invalid.
    let year = value
        .as_bytes()
        .windows(4)
        .filter_map(|digits| std::str::from_utf8(digits).ok())
        .filter_map(|digits| digits.parse::<i32>().ok())
        .find(|year| (1900..=2099).contains(year))?;
    NaiveDate::from_ymd_opt(year, 1, 1).map(|date| date.to_string())
}

#[cfg(test)]
mod tests {
    use super::normalize_publication_date;

    #[test]
    fn fills_missing_publication_date_parts() {
        assert_eq!(
            normalize_publication_date("2024"),
            Some("2024-01-01".into())
        );
        assert_eq!(
            normalize_publication_date("2024-06"),
            Some("2024-06-01".into())
        );
        assert_eq!(
            normalize_publication_date("2024-06-15"),
            Some("2024-06-15".into())
        );
        assert_eq!(
            normalize_publication_date("Published in 2024"),
            Some("2024-01-01".into())
        );
        assert_eq!(
            normalize_publication_date("2024-13"),
            Some("2024-01-01".into())
        );
        assert_eq!(
            normalize_publication_date("2024-02-30"),
            Some("2024-01-01".into())
        );
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
