//! Shared helpers of the D2b-F4 tests: constructed documents with sections and header parts, and
//! seeded command generators for the sections, header/footer, page and text-layout families
//! (plausible and hostile).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::ids::HfPartId;
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::ops::context::ContextCmd;
use uo_docx::session::ops::format::{ColorArg, ThemeColorRef};
use uo_docx::session::ops::header_footer::hf_templates::{HfGallery, hf_templates};
use uo_docx::session::ops::header_footer::{HeaderFooterCmd, HfAt, HfEnter, HfKind, HfReenter, HfSlot, PageNumberFormatPatch};
use uo_docx::session::ops::page::cover_templates::COVER_TEMPLATES;
use uo_docx::session::ops::page::watermark::WatermarkArg;
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
use uo_docx::write::emit::background::{WatermarkBox, WatermarkLayout, WatermarkPictureSpec, WatermarkTextSpec};
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
    match rng.below(112) {
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
        100..=111 => remainder_cmd(rng, s),
        _ => Command::Paragraph(ParagraphCmd::SetAlignment { alignment: "center".into() }),
    }
}

fn lang(rng: &mut Rng) -> PageLang {
    [PageLang::En, PageLang::Pt, PageLang::Es][rng.below(3)]
}

fn png() -> std::sync::Arc<[u8]> {
    std::sync::Arc::from(vec![0x89u8, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3].into_boxed_slice())
}

const WM_IDS: [&str; 6] = ["confidential1", "draft2", "donotcopy1", "asap2", "urgent1", "nope"];
const WM_TEXTS: [&str; 6] = ["Rascunho", "DRAFT", "A < B & \"C\"", " ", "Confidencial \u{1f600}", "X"];

/// `enter` of `insertHeaderFooterTemplate`: the part the layout would show (an existing one, or the one a
/// section creates).
fn enter(rng: &mut Rng, s: &Session) -> Option<HfEnter> {
    if rng.below(5) == 0 {
        return None;
    }
    let part = part(rng, s).unwrap_or(HfReenter::Create { kind: HfKind::Header, section: 0, slot: HfSlot::Default });
    Some(HfEnter { part, page: u32::try_from(rng.below(3)).unwrap() })
}

fn template_cmd(rng: &mut Rng, s: &Session, gallery: HfGallery, hostile: bool) -> Command {
    let list = hf_templates(gallery);
    let id = if rng.below(8) == 0 { "nope" } else { list[rng.below(list.len())].id };
    Command::HeaderFooter(HeaderFooterCmd::InsertTemplate {
        gallery,
        id: id.to_owned(),
        lang: lang(rng),
        enter: enter(rng, s),
        page_numbers: (rng.below(2) == 0)
            .then(|| if hostile { (hostile_f64(rng), hostile_f64(rng)) } else { (1.0 + rng.below(9) as f64, 12.0) }),
    })
}

/// A command of the watermark, cover page and header/footer template families (plausible).
pub fn remainder_cmd(rng: &mut Rng, s: &Session) -> Command {
    let flag = |rng: &mut Rng| rng.below(2) == 0;
    match rng.below(12) {
        0 => Command::Page(PageCmd::SetWatermark {
            spec: Box::new(WatermarkArg::Gallery(WM_IDS[rng.below(6)].to_owned())),
            at: at(rng, s),
            lang: lang(rng),
            reenter: part(rng, s),
        }),
        1 => Command::Page(PageCmd::SetWatermark {
            spec: Box::new(WatermarkArg::Text {
                spec: WatermarkTextSpec {
                    text: WM_TEXTS[rng.below(6)].to_owned(),
                    font: ["calibri", "Arial"][rng.below(2)].to_owned(),
                    size_pt: flag(rng).then(|| [20.0, 48.5, 0.0][rng.below(3)]),
                    color: ["C0C0C0", "FF0000"][rng.below(2)].to_owned(),
                    semitransparent: flag(rng),
                    layout: if flag(rng) { WatermarkLayout::Diagonal } else { WatermarkLayout::Horizontal },
                    lang: None,
                },
                measured_box: flag(rng).then_some(WatermarkBox { width_pt: 200.0, height_pt: 40.0 }),
            }),
            at: at(rng, s),
            lang: lang(rng),
            reenter: part(rng, s),
        }),
        2 => Command::Page(PageCmd::SetWatermark {
            spec: Box::new(WatermarkArg::Picture {
                spec: WatermarkPictureSpec {
                    data: png(),
                    ext: ["png", "PNG", "jpg"][rng.below(3)].to_owned(),
                    title: "logo.png".to_owned(),
                    scale: flag(rng).then_some(50.0),
                    washout: flag(rng),
                },
                pixel_info: flag(rng).then_some([100.0, 50.0, 96.0]),
            }),
            at: at(rng, s),
            lang: lang(rng),
            reenter: part(rng, s),
        }),
        3 => Command::Page(PageCmd::RemoveWatermark { reenter: part(rng, s) }),
        4..=5 => Command::Page(PageCmd::InsertCoverPage {
            id: if rng.below(9) == 0 { "nope".to_owned() } else { COVER_TEMPLATES[rng.below(8)].id.to_owned() },
            lang: lang(rng),
        }),
        6 => Command::Page(PageCmd::RemoveCoverPage),
        7..=10 => {
            let g = [HfGallery::Header, HfGallery::Footer, HfGallery::PageTop, HfGallery::PageBottom, HfGallery::PageMargin][rng.below(5)];
            template_cmd(rng, s, g, false)
        }
        _ => template_cmd(rng, s, HfGallery::PageCurrent, false),
    }
}

/// A command of the remainder with hostile arguments: ids that do not exist, sizes NaN/infinite, boxes
/// and pixel sizes of zero, extensions that are not extensions, parts and sections past the end, a
/// text of 200 KB.
pub fn hostile_remainder_cmd(rng: &mut Rng, s: &Session) -> Command {
    let n = hostile_f64(rng);
    let page_set = |spec: WatermarkArg, rng: &mut Rng| {
        Command::Page(PageCmd::SetWatermark {
            spec: Box::new(spec),
            at: HfAt { section: [0, 1, u32::MAX][rng.below(3)], slot: [HfSlot::Default, HfSlot::First, HfSlot::Even][rng.below(3)] },
            lang: lang(rng),
            reenter: [
                None,
                Some(HfReenter::Part(HfPartId(u32::MAX))),
                Some(HfReenter::Create { kind: HfKind::Header, section: u32::MAX, slot: HfSlot::First }),
            ][rng.below(3)],
        })
    };
    match rng.below(11) {
        0 => page_set(WatermarkArg::Gallery([String::new(), "nope".to_owned(), "\u{0}".to_owned()][rng.below(3)].clone()), rng),
        1 => page_set(
            WatermarkArg::Text {
                spec: WatermarkTextSpec {
                    text: "a".repeat(200_000),
                    font: "x\"<&>".to_owned(),
                    size_pt: Some(n),
                    color: "zz".to_owned(),
                    semitransparent: true,
                    layout: WatermarkLayout::Diagonal,
                    lang: None,
                },
                measured_box: Some(WatermarkBox { width_pt: n, height_pt: hostile_f64(rng) }),
            },
            rng,
        ),
        2 => page_set(
            WatermarkArg::Picture {
                spec: WatermarkPictureSpec {
                    data: png(),
                    ext: ["", "../x", "jp(g", "$&", "png", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"][rng.below(6)].to_owned(),
                    title: "t\"<".to_owned(),
                    scale: Some(n),
                    washout: true,
                },
                pixel_info: Some(
                    [[0.0, 0.0, 0.0], [f64::NAN, f64::NAN, f64::NAN], [1e9, 1.0, 1.0], [n, 1.0, hostile_f64(rng)]][rng.below(4)],
                ),
            },
            rng,
        ),
        3 => Command::Page(PageCmd::RemoveWatermark { reenter: Some(HfReenter::Part(HfPartId(u32::MAX))) }),
        4 => Command::Page(PageCmd::InsertCoverPage { id: ["", "nope", "\u{0}", "BAND"][rng.below(4)].to_owned(), lang: lang(rng) }),
        5 => Command::HeaderFooter(HeaderFooterCmd::InsertTemplate {
            gallery: HfGallery::ALL[rng.below(6)],
            id: ["", "nope", "\u{0}"][rng.below(3)].to_owned(),
            lang: lang(rng),
            enter: Some(HfEnter { part: HfReenter::Part(HfPartId(u32::MAX)), page: u32::MAX }),
            page_numbers: Some((n, hostile_f64(rng))),
        }),
        6 => Command::HeaderFooter(HeaderFooterCmd::InsertTemplate {
            gallery: HfGallery::ALL[rng.below(6)],
            id: "blank".to_owned(),
            lang: lang(rng),
            enter: Some(HfEnter { part: HfReenter::Create { kind: HfKind::Footer, section: u32::MAX, slot: HfSlot::Even }, page: 0 }),
            page_numbers: None,
        }),
        7 => {
            let g = HfGallery::ALL[rng.below(6)];
            template_cmd(rng, s, g, true)
        }
        8 => Command::Page(PageCmd::RemoveCoverPage),
        _ => remainder_cmd(rng, s),
    }
}

/// A document with document properties (`dc:title`, `dc:creator`, `Company`) for the cover pages.
pub fn doc_with_props() -> Vec<u8> {
    let mut pkg = uo_opc::Package::open(&doc(3, 1), &uo_opc::Limits::default(), &uo_opc::package::OpenOptions::default()).unwrap();
    pkg.set_part_text(
        "docProps/core.xml",
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>My &amp; Title</dc:title><dc:creator> Eric </dc:creator></cp:coreProperties>",
    );
    pkg.set_part_text(
        "docProps/app.xml",
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Company>ACME</Company></Properties>",
    );
    pkg.save(&uo_opc::package::SaveOptions::default()).unwrap()
}

/// A document whose `word/header9.xml` holds a watermark (the output of the W5 emitter).
pub fn doc_with_watermark() -> Vec<u8> {
    use uo_docx::write::emit::background::{WatermarkIds, WatermarkSpec, vml_color_attr, watermark_run_xml};
    let mut pkg = uo_opc::Package::open(&doc(3, 1), &uo_opc::Limits::default(), &uo_opc::package::OpenOptions::default()).unwrap();
    let spec = WatermarkSpec::Text(WatermarkTextSpec {
        text: "DRAFT".to_owned(),
        font: "calibri".to_owned(),
        size_pt: None,
        color: "C0C0C0".to_owned(),
        semitransparent: true,
        layout: WatermarkLayout::Diagonal,
        lang: None,
    });
    let ids = WatermarkIds { n: 123_456_789.0, spid: 2049.0, anchor_id: "0A0B0C0D".to_owned() };
    let run = watermark_run_xml(&spec, WatermarkBox { width_pt: 412.4, height_pt: 247.45 }, &ids, None, Some(&vml_color_attr));
    pkg.set_part_text(
        "word/header9.xml",
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:hdr {W} xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns:w10=\"urn:schemas-microsoft-com:office:word\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\"><w:p>{run}<w:r><w:t>Header text</w:t></w:r></w:p></w:hdr>"
        ),
    );
    pkg.save(&uo_opc::package::SaveOptions::default()).unwrap()
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
        0 if rng.below(3) == 0 => hostile_remainder_cmd(rng, s),
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
