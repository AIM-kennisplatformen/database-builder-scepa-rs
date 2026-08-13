//! Body passage extraction, whitespace normalization, and reference offsets.

use crate::models::draft::{FormulaPassage, Passage, PassageLevel, ReferenceSpan, TextPassage};

use super::{
    Counters,
    common::{non_empty_text, parse_coordinates},
    xml::{XmlElement, XmlNode, clean_text},
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
                if let Some(text) = non_empty_text(element) {
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
    let (text, references) = text_and_references(element);
    debug_assert!(references.iter().all(|reference| {
        text.get(reference.byte_start..reference.byte_end) == Some(reference.text.as_str())
    }));
    TextPassage {
        id: element.attr("id").map(str::to_owned).unwrap_or(fallback_id),
        text,
        coordinates: parse_coordinates(element.attr("coords")),
        references,
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

    fn push_atomic(&mut self, text: &str) -> (usize, usize) {
        let text = clean_text(text);
        if text.is_empty() {
            return (self.output.len(), self.output.len());
        }
        if self.pending_space && !self.output.is_empty() {
            self.output.push(' ');
        }
        self.pending_space = false;
        let start = self.output.len();
        self.output.push_str(&text);
        (start, self.output.len())
    }
}

fn text_and_references(element: &XmlElement) -> (String, Vec<ReferenceSpan>) {
    fn walk(element: &XmlElement, text: &mut NormalizedText, references: &mut Vec<ReferenceSpan>) {
        for child in &element.children {
            match child {
                XmlNode::Text(value) => text.push_text(value),
                XmlNode::Element(child)
                    if child.name == "ref" && child.attr("type") == Some("bibr") =>
                {
                    let reference_text = child.text();
                    let (byte_start, byte_end) = text.push_atomic(&reference_text);
                    if byte_start < byte_end {
                        references.push(ReferenceSpan {
                            target: child.attr("target").map(str::to_owned),
                            text: reference_text,
                            byte_start,
                            byte_end,
                        });
                    }
                }
                XmlNode::Element(child) => walk(child, text, references),
            }
        }
    }

    let mut text = NormalizedText::default();
    let mut references = Vec::new();
    walk(element, &mut text, &mut references);
    (text.output, references)
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
