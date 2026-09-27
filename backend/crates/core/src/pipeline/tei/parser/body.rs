//! Body passage extraction and whitespace normalization.

use crate::models::draft::{FormulaPassage, Passage, PassageLevel, TextPassage};

use super::{
    Counters,
    common::{non_empty_text, parse_coordinates},
    xml::{XmlElement, XmlNode},
};

pub(super) fn parse_body(
    text: &XmlElement,
    level: PassageLevel,
    counters: &mut Counters,
    output: &mut Vec<Passage>,
) {
    for section in text.children.iter().filter_map(|node| match node {
        XmlNode::Element(element) if element.name == "body" || element.name == "back" => {
            Some(element)
        }
        _ => None,
    }) {
        let mut heading_context = None;
        for div in section.children_named("div") {
            if div.attr("type") == Some("references") {
                continue;
            }
            let has_content = div.child("p").is_some() || div.child("formula").is_some();
            let has_nested_div = div.child("div").is_some();
            if !has_content && !has_nested_div {
                if let Some(head) = div.child("head").and_then(non_empty_text) {
                    heading_context = Some(head);
                }
                continue;
            }
            parse_div(div, level, heading_context.as_deref(), counters, output);
            heading_context = None;
        }
    }
}

fn parse_div(
    div: &XmlElement,
    level: PassageLevel,
    heading_context: Option<&str>,
    counters: &mut Counters,
    output: &mut Vec<Passage>,
) {
    if div.attr("type") == Some("references") {
        return;
    }
    let has_content = div.child("p").is_some() || div.child("formula").is_some();
    let nested: Vec<_> = div.children_named("div").collect();
    if !nested.is_empty() && !has_content {
        for child in nested {
            parse_div(child, level, None, counters, output);
        }
        return;
    }

    let section = div
        .child("head")
        .and_then(non_empty_text)
        .or_else(|| div.attr("type").map(section_name));

    for child in &div.children {
        let XmlNode::Element(element) = child else {
            continue;
        };
        match element.name.as_str() {
            "p" => match level {
                PassageLevel::Paragraph => {
                    counters.passage += 1;
                    let id = element
                        .attr("id")
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("p_{:08}", counters.passage));
                    output.push(Passage::Text(parse_text_passage(
                        element,
                        id,
                        heading_context.map(str::to_owned),
                        section.clone(),
                    )));
                }
                PassageLevel::Sentence => {
                    for sentence in element.descendants_named("s") {
                        counters.passage += 1;
                        let id = sentence
                            .attr("id")
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("s_{:08}", counters.passage));
                        output.push(Passage::Text(parse_text_passage(
                            sentence,
                            id,
                            heading_context.map(str::to_owned),
                            section.clone(),
                        )));
                    }
                }
            },
            "formula" => {
                let text = text_without_citations(element);
                if !text.is_empty() {
                    counters.formula += 1;
                    output.push(Passage::Formula(FormulaPassage {
                        id: element
                            .attr("id")
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("f_{:08}", counters.formula)),
                        text,
                        label: element.child("label").and_then(non_empty_text),
                        coordinates: parse_coordinates(element.attr("coords")),
                        heading_context: heading_context.map(str::to_owned),
                        section: section.clone(),
                    }));
                }
            }
            _ => {}
        }
    }
}

pub(super) fn parse_text_passage(
    element: &XmlElement,
    fallback_id: String,
    heading_context: Option<String>,
    section: Option<String>,
) -> TextPassage {
    TextPassage {
        id: element.attr("id").map(str::to_owned).unwrap_or(fallback_id),
        text: text_without_citations(element),
        coordinates: parse_coordinates(element.attr("coords")),
        heading_context,
        section,
    }
}

#[derive(Default)]
struct NormalizedText {
    output: String,
    pending_space: bool,
}

impl NormalizedText {
    fn push_text(&mut self, text: &str) {
        for character in text.chars() {
            if character.is_whitespace() {
                self.pending_space = true;
            } else {
                if self.pending_space && !self.output.is_empty() {
                    self.output.push(' ');
                }
                self.pending_space = false;
                self.output.push(character);
            }
        }
    }
}

fn text_without_citations(element: &XmlElement) -> String {
    fn walk(element: &XmlElement, text: &mut NormalizedText) {
        for child in &element.children {
            match child {
                XmlNode::Text(value) => text.push_text(value),
                XmlNode::Element(child)
                    if child.name == "ref" && child.attr("type") == Some("bibr") => {}
                XmlNode::Element(child) => walk(child, text),
            }
        }
    }

    let mut text = NormalizedText::default();
    walk(element, &mut text);
    text.output
}

fn section_name(value: &str) -> String {
    match value {
        "acknowledgement" => "Acknowledgements".into(),
        "conflict" => "Conflicts of Interest".into(),
        "contribution" => "Author Contributions".into(),
        "availability" => "Data Availability".into(),
        "annex" => "Annex".into(),
        value => value
            .split('_')
            .map(|word| {
                let mut characters = word.chars();
                characters
                    .next()
                    .map(|first| first.to_uppercase().chain(characters).collect())
                    .unwrap_or_default()
            })
            .collect::<Vec<String>>()
            .join(" "),
    }
}
