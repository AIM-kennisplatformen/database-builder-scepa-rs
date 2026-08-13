//! TEI XML parsing and typed extraction.

mod body;
mod citation;
mod common;
mod media;
mod metadata;
mod xml;

use crate::models::draft::{PassageLevel, TeiDocument};

#[derive(Default)]
struct Counters {
    passage: usize,
    formula: usize,
    media: usize,
}

pub(super) fn convert_tei(tei: &str) -> eros::Result<TeiDocument> {
    let root = match xml::parse(tei) {
        Ok(root) => root,
        Err(error) => eros::bail!("invalid TEI XML: {}", error),
    };
    if root.name != "TEI" {
        eros::bail!("expected a TEI root element, found <{}>", root.name)
    }

    let sentence_count = root.descendants_named("s").len();
    let paragraph_count = root.descendants_named("p").len();
    let level = if sentence_count > paragraph_count {
        PassageLevel::Sentence
    } else {
        PassageLevel::Paragraph
    };

    let bibliography = root
        .child("teiHeader")
        .map(|header| metadata::parse_bibliography(header, level))
        .unwrap_or_default();

    let mut counters = Counters::default();
    let mut body_text = Vec::new();
    let mut figures_and_tables = Vec::new();
    if let Some(text) = root.child("text") {
        body::parse_body(text, level, &mut counters, &mut body_text);
        figures_and_tables = media::parse_figures(text, &mut counters);
    }

    let references = root
        .descendants_named("listBibl")
        .into_iter()
        .flat_map(|list| list.descendants_named("biblStruct"))
        .enumerate()
        .filter_map(|(index, entry)| citation::parse_citation(entry, index + 1))
        .collect();

    Ok(TeiDocument {
        level,
        bibliography,
        body_text,
        figures_and_tables,
        references,
    })
}

#[cfg(test)]
mod tests {
    use crate::models::draft::{FigureOrTable, Passage};

    use super::*;

    const SAMPLE: &str = r##"
        <TEI xmlns="http://www.tei-c.org/ns/1.0">
          <teiHeader>
            <fileDesc>
              <titleStmt>
                <title type="main" level="a">Typed TEI</title>
                <author><persName><forename>Ada</forename><surname>Lovelace</surname></persName></author>
              </titleStmt>
              <publicationStmt><publisher>Example Press</publisher></publicationStmt>
              <sourceDesc><biblStruct><monogr><title type="main" level="j">Computing</title></monogr></biblStruct></sourceDesc>
            </fileDesc>
            <profileDesc><abstract><p coords="1,10,20,30,40">An abstract.</p></abstract></profileDesc>
            <idno type="DOI">10.1/example</idno>
            <date type="published" when="2024-05-06"/>
          </teiHeader>
          <text>
            <body>
              <div><head>Introduction</head><p xml:id="p1" coords="1,1,2,3,4">See <ref type="bibr" target="#b1">[1]</ref> for details.</p></div>
              <div><formula xml:id="eq1"><label>(1)</label>x = y</formula></div>
              <figure xml:id="fig1"><head>A figure</head><graphic coords="2,5,6,7,8"/></figure>
              <figure type="table" xml:id="tab1"><table><row><cell>A</cell><cell>B</cell></row></table></figure>
            </body>
            <back><div type="references"><listBibl><biblStruct xml:id="b1">
              <analytic><title level="a">Prior work</title><author><forename>Grace</forename><surname>Hopper</surname></author><idno type="DOI">10.2/prior</idno></analytic>
              <monogr><title level="j">Journal</title><imprint><date when="2020"/><biblScope unit="page" from="1" to="9"/></imprint></monogr>
              <note type="raw_reference">Hopper, 2020</note><ptr target="https://example.test/paper"/>
            </biblStruct></listBibl></div></back>
          </text>
        </TEI>
    "##;

    #[test]
    fn converts_representative_tei_to_typed_document() {
        let document = convert_tei(SAMPLE).unwrap();
        assert_eq!(document.level, PassageLevel::Paragraph);
        assert_eq!(document.bibliography.title.as_deref(), Some("Typed TEI"));
        assert_eq!(document.bibliography.authors[0].name, "Ada Lovelace");
        assert_eq!(document.bibliography.publication_year, Some(2024));
        assert_eq!(document.body_text.len(), 2);

        let Passage::Text(passage) = &document.body_text[0] else {
            panic!()
        };
        assert_eq!(passage.text, "See [1] for details.");
        assert_eq!(passage.section.as_deref(), Some("Introduction"));
        assert_eq!(
            &passage.text[passage.references[0].byte_start..passage.references[0].byte_end],
            "[1]"
        );
        assert_eq!(passage.coordinates[0].page, Some(1));

        assert_eq!(document.figures_and_tables.len(), 2);
        let FigureOrTable::Table(table) = &document.figures_and_tables[1] else {
            panic!()
        };
        assert_eq!(table.content.as_ref().unwrap().rows, vec![vec!["A", "B"]]);

        assert_eq!(document.references.len(), 1);
        assert_eq!(document.references[0].title.as_deref(), Some("Prior work"));
        assert_eq!(document.references[0].publication.year, Some(2020));
        assert_eq!(document.references[0].contributors[0].name, "Grace Hopper");
    }

    #[test]
    fn selects_sentence_level_and_preserves_unicode_offsets() {
        let xml = r##"<TEI><teiHeader/><text><body><div><p><s>α <ref type="bibr" target="#b1">[é]</ref>.</s><s>Two.</s></p></div></body></text></TEI>"##;
        let document = convert_tei(xml).unwrap();
        assert_eq!(document.level, PassageLevel::Sentence);
        assert_eq!(document.body_text.len(), 2);
        let Passage::Text(first) = &document.body_text[0] else {
            panic!()
        };
        let reference = &first.references[0];
        assert_eq!(&first.text[reference.byte_start..reference.byte_end], "[é]");
    }

    #[test]
    fn rejects_non_tei_and_malformed_xml() {
        assert!(convert_tei("<root/>").is_err());
        assert!(convert_tei("<TEI><text></TEI>").is_err());
    }

    #[test]
    fn serializes_public_model() {
        let document = convert_tei(SAMPLE).unwrap();
        let json = serde_json::to_value(document).unwrap();
        assert_eq!(json["level"], "paragraph");
        assert_eq!(json["body_text"][0]["type"], "text");
        assert_eq!(json["figures_and_tables"][1]["type"], "table");
    }
}
