//! Shared helpers of the D2b-F4 tests: constructed documents with sections and header parts, and
//! seeded command generators for the sections, header/footer, page and text-layout families
//! (plausible and hostile).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::ids::HfPartId;
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::ops::context::ContextCmd;
use uo_docx::session::ops::format::{ColorArg, ThemeColorRef};
use uo_docx::session::ops::header_footer::{HeaderFooterCmd, HfAt, HfKind, HfReenter, HfSlot, PageNumberFormatPatch};
use uo_docx::session::ops::page::{GradientStyle, PageCmd, PageFillInput, PageLang};
use uo_docx::session::ops::paragraph::ParagraphCmd;
use uo_docx::session::ops::sections::section_ops::{ColumnPreset, ColumnSpec, Orientation};
use uo_docx::session::ops::sections::{
    ColumnsArg, MarginPreset, PageSetup, PageSize, SectionApplyTo, SectionCmd, SectionType, VerticalAlignment,
};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::ops::text_layout::{HyphenationPatch, LineNumberMode, LineNumberingOptions, LineRestart, TextLayoutCmd};
use uo_docx::session::{Command, DocPosition, Session};
use uo_docx::text::Utf16;
use uo_testkit::generate::Rng;

pub const W: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";

/// A document with `n` paragraphs of text, a section break after paragraph 0 (`sections` = 2) and
/// a header reference (`default`, rId90 -> `word/header9.xml`) on the first section; the last
/// section (the body's) has none, so it is linked to the previous.
pub fn doc(paras: usize, sections: usize) -> Vec<u8> {
    let mut body = String::new();
    for i in 0..paras {
        let sect = if sections > 1 && i == 0 {
            "<w:pPr><w:sectPr><w:headerReference w:type=\"default\" r:id=\"rId90\"/><w:type w:val=\"nextPage\"/><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:pPr>"
        } else {
            ""
        };
        body.push_str(&format!("<w:p>{sect}<w:r><w:t xml:space=\"preserve\">Paragraph {i} of the text</w:t></w:r></w:p>"));
    }
    let hdr = if sections > 1 { "" } else { "<w:headerReference w:type=\"default\" r:id=\"rId90\"/>" };
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document {W}><w:body>{body}<w:sectPr>{hdr}<w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
    );
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    pkg.set_part_text("word/document.xml", xml);
    let rels = pkg.part_text("word/_rels/document.xml.rels").unwrap().unwrap().into_owned();
    pkg.set_part_text(
        "word/_rels/document.xml.rels",
        rels.replace(
            "</Relationships>",
            "<Relationship Id=\"rId90\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/header\" Target=\"header9.xml\"/></Relationships>",
        ),
    );
    let ct = pkg.part_text("[Content_Types].xml").unwrap().unwrap().into_owned();
    pkg.set_part_text(
        "[Content_Types].xml",
        ct.replace(
            "</Types>",
            "<Override PartName=\"/word/header9.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/></Types>",
        ),
    );
    pkg.set_part_text(
        "word/header9.xml",
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:hdr {W}><w:p><w:r><w:t>Header text</w:t></w:r></w:p></w:hdr>"
        ),
    );
    pkg.save(&uo_opc::package::SaveOptions::default()).unwrap()
}

fn pos(rng: &mut Rng, s: &Session) -> DocPosition {
    let paras = s.paras().unwrap();
    let para = rng.below(paras.len());
    let len = paras.get(para).map_or(0, uo_docx::session::text::para_length);
    DocPosition::new(u32::try_from(para).unwrap(), u32::try_from(rng.below(len + 1)).unwrap())
}

fn apply(rng: &mut Rng) -> SectionApplyTo {
    [SectionApplyTo::Section, SectionApplyTo::Document, SectionApplyTo::Forward, SectionApplyTo::Selection][rng.below(4)]
}

fn kind_of(rng: &mut Rng) -> SectionType {
    [SectionType::NextPage, SectionType::Continuous, SectionType::NextColumn, SectionType::EvenPage, SectionType::OddPage][rng.below(5)]
}

fn at(rng: &mut Rng, s: &Session) -> HfAt {
    HfAt {
        section: u32::try_from(rng.below(s.document().sections.len() + 1)).unwrap(),
        slot: [HfSlot::Default, HfSlot::First, HfSlot::Even][rng.below(3)],
    }
}

fn part(rng: &mut Rng, s: &Session) -> Option<HfReenter> {
    let n = s.document().hf.len();
    match rng.below(5) {
        0 if n > 0 => Some(HfReenter::Part(HfPartId(u32::try_from(rng.below(n)).unwrap()))),
        1 => Some(HfReenter::Create {
            kind: if rng.below(2) == 0 { HfKind::Header } else { HfKind::Footer },
            section: u32::try_from(rng.below(3)).unwrap(),
            slot: [HfSlot::Default, HfSlot::First, HfSlot::Even][rng.below(3)],
        }),
        _ => None,
    }
}

fn hostile_f64(rng: &mut Rng) -> f64 {
    [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300, -1e300, 0.0, -0.0, 1.0, 720.0, 1e9][rng.below(10)]
}

/// A command of the F4 families a user could give now, mixed with typing and selections.
pub fn f4_cmd(rng: &mut Rng, s: &Session) -> Command {
    let flag = |rng: &mut Rng| rng.below(2) == 0;
    match rng.below(100) {
        0..=9 => Command::Text(TextCmd::Type { text: Utf16::from(["a", "Olá ", "😀"][rng.below(3)]) }),
        10..=12 => Command::Text(TextCmd::Enter),
        13..=15 => Command::Text(TextCmd::Backspace),
        16..=23 => {
            let a = pos(rng, s);
            let f = if rng.below(2) == 0 { a } else { pos(rng, s) };
            Command::Text(TextCmd::SetSelection { anchor: a, focus: f })
        }
        24..=26 => Command::Settings(SettingsCmd::SetTracking { on: flag(rng) }),
        27..=34 => Command::Section(SectionCmd::SetPageSetup(Box::new(PageSetup {
            orientation: flag(rng).then(|| if flag(rng) { Orientation::Landscape } else { Orientation::Portrait }),
            size: flag(rng).then(|| if flag(rng) { PageSize::A4 } else { PageSize::Letter }),
            margins: flag(rng).then(|| [MarginPreset::Normal, MarginPreset::Narrow, MarginPreset::Wide][rng.below(3)]),
            custom_margins: (rng.below(4) == 0).then_some([1000.0, 1100.0, 1200.0, 1300.0]),
            section_start: flag(rng).then(|| kind_of(rng)),
            vertical_alignment: flag(rng).then(|| {
                [VerticalAlignment::Top, VerticalAlignment::Center, VerticalAlignment::Both, VerticalAlignment::Bottom][rng.below(4)]
            }),
            line_numbering: flag(rng).then(|| LineNumberingOptions {
                on: flag(rng),
                start_at: 1.0 + rng.below(5) as f64,
                count_by: 1.0 + rng.below(3) as f64,
                distance_twips: [0.0, 300.0][rng.below(2)],
                restart: [LineRestart::Page, LineRestart::Section, LineRestart::Continuous][rng.below(3)],
            }),
            apply_to: flag(rng).then(|| apply(rng)),
        }))),
        35..=40 => Command::Section(SectionCmd::InsertSectionBreak { kind: kind_of(rng) }),
        41..=43 => Command::Section(SectionCmd::InsertColumnBreak),
        44..=47 => Command::Section(SectionCmd::InsertPageBreak),
        48..=49 => Command::Section(SectionCmd::InsertBlankPage),
        50..=56 => Command::Section(SectionCmd::SetColumns(Box::new(if flag(rng) {
            ColumnsArg::Preset(
                [ColumnPreset::One, ColumnPreset::Two, ColumnPreset::Three, ColumnPreset::Left, ColumnPreset::Right][rng.below(5)],
            )
        } else {
            ColumnsArg::Spec {
                spec: ColumnSpec {
                    count: 1.0 + rng.below(4) as f64,
                    space_twips: flag(rng).then(|| 300.0 * rng.below(6) as f64),
                    widths: (rng.below(3) == 0).then(|| vec![(3000.0, 300.0), (2500.0, 200.0), (2000.0, 100.0), (900.0, 0.0)]),
                    sep: flag(rng),
                },
                apply_to: flag(rng).then(|| apply(rng)),
                start_new_column: flag(rng),
            }
        }))),
        57..=59 => Command::Section(SectionCmd::SetSectionStart { kind: kind_of(rng), apply_to: apply(rng), hf_section: None }),
        60..=61 => Command::Section(SectionCmd::SetVerticalAlignment {
            v: [VerticalAlignment::Top, VerticalAlignment::Center, VerticalAlignment::Both, VerticalAlignment::Bottom][rng.below(4)],
            apply_to: apply(rng),
            hf_section: None,
        }),
        62..=64 => Command::Section(SectionCmd::SetLineNumbering {
            mode: [LineNumberMode::None, LineNumberMode::Continuous, LineNumberMode::Page, LineNumberMode::Section][rng.below(4)],
            hf_section: None,
        }),
        65 => Command::Section(SectionCmd::ToggleLineNumberSuppression),
        66..=67 => Command::Section(SectionCmd::SetPageBorders { borders: None, document_wide: flag(rng) }),
        68..=70 => Command::HeaderFooter(HeaderFooterCmd::SetDifferentFirstPage { on: flag(rng), at: at(rng, s), reenter: part(rng, s) }),
        71..=72 => Command::HeaderFooter(HeaderFooterCmd::SetDifferentOddEven { on: flag(rng), reenter: part(rng, s) }),
        73..=75 => Command::HeaderFooter(HeaderFooterCmd::SetHeaderFooterDistance {
            header: flag(rng).then(|| 100.0 * rng.below(9) as f64),
            footer: flag(rng).then(|| 100.0 * rng.below(9) as f64),
            at: at(rng, s),
            reenter: part(rng, s),
        }),
        76..=79 => Command::HeaderFooter(HeaderFooterCmd::SetLinkToPrevious {
            on: flag(rng),
            kind: flag(rng).then(|| if flag(rng) { HfKind::Header } else { HfKind::Footer }),
            at: at(rng, s),
            reenter: part(rng, s),
        }),
        80..=81 => Command::HeaderFooter(HeaderFooterCmd::RemoveHeaderFooter {
            kind: if flag(rng) { HfKind::Header } else { HfKind::Footer },
            at: at(rng, s),
        }),
        82 => Command::HeaderFooter(HeaderFooterCmd::RemovePageNumbers { at: at(rng, s) }),
        83 => Command::HeaderFooter(HeaderFooterCmd::SetPageNumberFormat {
            f: PageNumberFormatPatch {
                format: flag(rng).then(|| ["decimal", "lowerRoman", "upperLetter"][rng.below(3)].to_owned()),
                chapter: flag(rng).then(|| flag(rng).then(|| (1.0 + rng.below(9) as f64, "hyphen".to_owned()))),
                start: flag(rng).then(|| flag(rng).then(|| rng.below(5) as f64)),
            },
            at: at(rng, s),
            reenter: part(rng, s),
        }),
        84 => Command::HeaderFooter(HeaderFooterCmd::CreateHeaderFooterPart {
            kind: if flag(rng) { HfKind::Header } else { HfKind::Footer },
            section: u32::try_from(rng.below(3)).unwrap(),
            slot: [HfSlot::Default, HfSlot::First, HfSlot::Even][rng.below(3)],
        }),
        85..=87 => Command::Context(ContextCmd::EnterHeaderFooter {
            part: HfPartId(u32::try_from(rng.below(s.document().hf.len() + 1)).unwrap()),
            page: 0,
        }),
        88 => Command::Context(ContextCmd::ExitHeaderFooter),
        89..=91 => Command::Page(PageCmd::SetPageColor {
            color: match rng.below(4) {
                0 => None,
                1 => Some(ColorArg::Hex(["#ffcc00", "336699", "AABBCC"][rng.below(3)].to_owned())),
                _ => Some(ColorArg::Theme(ThemeColorRef {
                    theme_color: ["accent1", "text2", "background1"][rng.below(3)].to_owned(),
                    tint: flag(rng).then_some(0.4),
                    shade: flag(rng).then_some(0.25),
                })),
            },
        }),
        92..=93 => Command::Page(PageCmd::SetPageFill {
            fill: match rng.below(6) {
                0 => None,
                1 => Some(PageFillInput::GradientOne {
                    color: "4472C4".into(),
                    degree: [0.0, 0.5, 0.8][rng.below(3)],
                    style: gs(rng),
                    variant: 1.0 + rng.below(4) as f64,
                }),
                2 => Some(PageFillInput::GradientTwo {
                    color: "4472C4".into(),
                    color2: "FFC000".into(),
                    style: gs(rng),
                    variant: 1.0 + rng.below(4) as f64,
                }),
                3 => Some(PageFillInput::GradientPreset {
                    preset: ["mist", "aurora", "none"][rng.below(3)].into(),
                    style: gs(rng),
                    variant: 1.0 + rng.below(4) as f64,
                }),
                4 => Some(PageFillInput::Pattern {
                    pattern: ["pct50", "wave", "nope"][rng.below(3)].into(),
                    color: "FF0000".into(),
                    background: "00FF00".into(),
                }),
                _ => Some(PageFillInput::Picture {
                    texture: flag(rng),
                    data: std::sync::Arc::from(vec![1u8, 2, 3].into_boxed_slice()),
                    ext: "PNG".into(),
                    name: "a.b.png".into(),
                }),
            },
            lang: [PageLang::En, PageLang::Pt, PageLang::Es][rng.below(3)],
        }),
        94 => Command::TextLayout(TextLayoutCmd::SetDefaultTabStop { twips: 100.0 + rng.below(1000) as f64 }),
        95 => Command::TextLayout(TextLayoutCmd::SetHyphenation { auto: flag(rng) }),
        96 => Command::TextLayout(TextLayoutCmd::SetHyphenationOptions {
            o: HyphenationPatch {
                auto: flag(rng).then(|| flag(rng)),
                caps: flag(rng).then(|| flag(rng)),
                zone_twips: flag(rng).then(|| 100.0 * rng.below(5) as f64),
                limit: flag(rng).then(|| rng.below(4) as f64),
            },
        }),
        _ => Command::Paragraph(ParagraphCmd::SetAlignment { alignment: "center".into() }),
    }
}

fn gs(rng: &mut Rng) -> GradientStyle {
    [
        GradientStyle::Horizontal,
        GradientStyle::Vertical,
        GradientStyle::DiagonalUp,
        GradientStyle::DiagonalDown,
        GradientStyle::FromCorner,
        GradientStyle::FromCenter,
    ][rng.below(6)]
}

/// A command with hostile numbers and indices.
pub fn hostile_cmd(rng: &mut Rng, s: &Session) -> Command {
    let n = hostile_f64(rng);
    match rng.below(14) {
        0 => Command::Section(SectionCmd::SetPageSetup(Box::new(PageSetup {
            custom_margins: Some([n, n, hostile_f64(rng), hostile_f64(rng)]),
            apply_to: Some(apply(rng)),
            ..PageSetup::default()
        }))),
        1 => Command::Section(SectionCmd::SetColumns(Box::new(ColumnsArg::Spec {
            spec: ColumnSpec {
                count: n,
                space_twips: Some(hostile_f64(rng)),
                widths: Some(vec![(n, n), (hostile_f64(rng), hostile_f64(rng))]),
                sep: true,
            },
            apply_to: Some(apply(rng)),
            start_new_column: true,
        }))),
        2 => Command::Section(SectionCmd::SetLineNumberingOptions {
            o: LineNumberingOptions {
                on: true,
                start_at: n,
                count_by: hostile_f64(rng),
                distance_twips: hostile_f64(rng),
                restart: LineRestart::Section,
            },
            apply_to: apply(rng),
            hf_section: Some(u32::MAX),
        }),
        3 => Command::HeaderFooter(HeaderFooterCmd::SetHeaderFooterDistance {
            header: Some(n),
            footer: Some(hostile_f64(rng)),
            at: HfAt { section: u32::MAX, slot: HfSlot::First },
            reenter: Some(HfReenter::Part(HfPartId(u32::MAX))),
        }),
        4 => Command::HeaderFooter(HeaderFooterCmd::SetPageNumberFormat {
            f: PageNumberFormatPatch {
                format: Some("x\u{0}y".into()),
                chapter: Some(Some((n, "sep".into()))),
                start: Some(Some(hostile_f64(rng))),
            },
            at: HfAt { section: 7, slot: HfSlot::Even },
            reenter: None,
        }),
        5 => Command::HeaderFooter(HeaderFooterCmd::SetLinkToPrevious {
            on: false,
            kind: None,
            at: HfAt { section: u32::MAX, slot: HfSlot::Default },
            reenter: Some(HfReenter::Part(HfPartId(u32::MAX))),
        }),
        6 => {
            Command::HeaderFooter(HeaderFooterCmd::CreateHeaderFooterPart { kind: HfKind::Footer, section: u32::MAX, slot: HfSlot::First })
        }
        7 => Command::Context(ContextCmd::EnterHeaderFooter { part: HfPartId(u32::try_from(rng.below(4)).unwrap()), page: u32::MAX }),
        8 => Command::TextLayout(TextLayoutCmd::SetDefaultTabStop { twips: n }),
        9 => Command::TextLayout(TextLayoutCmd::SetHyphenationOptions {
            o: HyphenationPatch { auto: Some(true), caps: None, zone_twips: Some(n), limit: Some(hostile_f64(rng)) },
        }),
        10 => Command::Page(PageCmd::SetPageFill {
            fill: Some(PageFillInput::GradientOne { color: "\u{0}".into(), degree: n, style: gs(rng), variant: hostile_f64(rng) }),
            lang: PageLang::En,
        }),
        11 => Command::Section(SectionCmd::SetSectionStart { kind: kind_of(rng), apply_to: apply(rng), hf_section: Some(u32::MAX) }),
        12 => Command::Section(SectionCmd::InsertSectionBreak { kind: kind_of(rng) }),
        _ => f4_cmd(rng, s),
    }
}
