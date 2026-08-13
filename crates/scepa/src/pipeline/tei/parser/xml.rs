//! Minimal namespace-agnostic XML tree used by the TEI extractors.

use std::collections::BTreeMap;

use quick_xml::{Reader, events::Event};

#[derive(Clone, Debug)]
pub(super) enum XmlNode {
    Element(XmlElement),
    Text(String),
}

#[derive(Clone, Debug)]
pub(super) struct XmlElement {
    pub(super) name: String,
    attributes: BTreeMap<String, String>,
    pub(super) children: Vec<XmlNode>,
}

impl XmlElement {
    pub(super) fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }

    pub(super) fn child(&self, name: &str) -> Option<&Self> {
        self.children.iter().find_map(|child| match child {
            XmlNode::Element(element) if element.name == name => Some(element),
            _ => None,
        })
    }

    pub(super) fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Self> {
        self.children.iter().filter_map(move |child| match child {
            XmlNode::Element(element) if element.name == name => Some(element),
            _ => None,
        })
    }

    pub(super) fn descendants_named<'a>(&'a self, name: &'a str) -> Vec<&'a Self> {
        let mut found = Vec::new();
        self.collect_descendants(name, &mut found);
        found
    }

    fn collect_descendants<'a>(&'a self, name: &str, found: &mut Vec<&'a Self>) {
        for child in &self.children {
            if let XmlNode::Element(element) = child {
                if element.name == name {
                    found.push(element);
                }
                element.collect_descendants(name, found);
            }
        }
    }

    pub(super) fn text(&self) -> String {
        let mut raw = String::new();
        self.append_raw_text(&mut raw);
        clean_text(&raw)
    }

    fn append_raw_text(&self, output: &mut String) {
        for child in &self.children {
            match child {
                XmlNode::Text(text) => output.push_str(text),
                XmlNode::Element(element) => element.append_raw_text(output),
            }
        }
    }
}

pub(super) fn parse(xml: &str) -> Result<XmlElement, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut stack = Vec::<XmlElement>::new();
    let mut root = None;

    loop {
        match reader.read_event().map_err(|error| error.to_string())? {
            Event::Start(start) => stack.push(element_from_start(&start)?),
            Event::Empty(start) => {
                let element = element_from_start(&start)?;
                append_element(&mut stack, &mut root, element)?;
            }
            Event::Text(text) => {
                let raw = std::str::from_utf8(text.as_ref()).map_err(|error| error.to_string())?;
                let decoded = quick_xml::escape::unescape(raw)
                    .map_err(|error| error.to_string())?
                    .into_owned();
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(XmlNode::Text(decoded));
                }
            }
            Event::CData(text) => {
                let decoded = String::from_utf8(text.into_inner().to_vec())
                    .map_err(|error| error.to_string())?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(XmlNode::Text(decoded));
                }
            }
            Event::End(_) => {
                let element = stack
                    .pop()
                    .ok_or_else(|| "encountered an unmatched closing tag".to_owned())?;
                append_element(&mut stack, &mut root, element)?;
            }
            Event::Eof => break,
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) | Event::DocType(_) => {}
            _ => {}
        }
    }

    if !stack.is_empty() {
        return Err("document ended before all elements were closed".into());
    }
    root.ok_or_else(|| "document contains no root element".into())
}

fn element_from_start(start: &quick_xml::events::BytesStart<'_>) -> Result<XmlElement, String> {
    let name = local_name(start.name().as_ref())?;
    let mut attributes = BTreeMap::new();
    for attribute in start.attributes().with_checks(false) {
        let attribute = attribute.map_err(|error| error.to_string())?;
        let key = local_name(attribute.key.as_ref())?;
        let raw =
            std::str::from_utf8(attribute.value.as_ref()).map_err(|error| error.to_string())?;
        let value = quick_xml::escape::unescape(raw)
            .map_err(|error| error.to_string())?
            .into_owned();
        attributes.insert(key, value);
    }
    Ok(XmlElement {
        name,
        attributes,
        children: Vec::new(),
    })
}

fn local_name(name: &[u8]) -> Result<String, String> {
    let name = std::str::from_utf8(name).map_err(|error| error.to_string())?;
    Ok(name.rsplit(':').next().unwrap_or(name).to_owned())
}

fn append_element(
    stack: &mut [XmlElement],
    root: &mut Option<XmlElement>,
    element: XmlElement,
) -> Result<(), String> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(XmlNode::Element(element));
    } else if root.replace(element).is_some() {
        return Err("document contains more than one root element".into());
    }
    Ok(())
}

pub(super) fn clean_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
