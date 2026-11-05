//! Shared helpers of the D2b-F2 tests (lists and styles): a seeded generator of F2 commands,
//! valid and hostile, and documents with hostile `numbering.xml` / `styles.xml`.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::session::ops::lists::{ListCmd, ListLevelFormat, ListLevelSpec};
use uo_docx::session::ops::styles::{Ch, CreateStyleSpec, DefaultFont, ModifyStyleSpec, StyleCmd, StyleFormat};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, DocPosition, Session};
use uo_docx::text::Utf16;
use uo_testkit::generate::Rng;

const IDS: [&str; 12] = [
    "Normal",
    "Ttulo1",
    "Ttulo2",
    "Heading1",
    "PargrafodaLista",
    "ListParagraph",
    "MeuEstilo",
    "MeuEstiloChar",
    "NoSuch",
    "",
    "a\"<b>&",
    "Fontepargpadro",
];
const NAMES: [&str; 8] = ["Meu Estilo", "Destaque X", "  ", "", "a\"<b>&c", "😀 emoji", "Ttulo 1", "heading 1"];

fn num(rng: &mut Rng, wild: bool) -> f64 {
    if wild {
        [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, 8.0, 9.0, 1e300, 2.5, -0.0][rng.below(10)]
    } else {
        rng.below(9) as f64
    }
}

fn level(rng: &mut Rng, wild: bool) -> ListLevelSpec {
    ListLevelSpec {
        ilvl: num(rng, wild),
        num_fmt: ["decimal", "upperRoman", "bullet", "a\"b"][rng.below(4)].to_owned(),
        lvl_text: ["%1.", "%1.%2.", "", "<&>\""][rng.below(4)].to_owned(),
        ind_left: num(rng, wild) * 360.0,
        hanging: num(rng, wild) * 90.0 - 90.0,
        p_style: (rng.below(4) == 0).then(|| IDS[rng.below(IDS.len())].to_owned()),
        start: (rng.below(3) == 0).then(|| num(rng, wild)),
        font: (rng.below(3) == 0).then(|| "Arial".to_owned()),
        color: (rng.below(3) == 0).then(|| ["FF0000", "zzzzzz", "ff00", "#00FF00"][rng.below(4)].to_owned()),
        size_pt: (rng.below(3) == 0).then(|| num(rng, wild)),
        is_lgl: rng.below(5) == 0,
        suff: (rng.below(4) == 0).then(|| "space".to_owned()),
        tab_pos: (rng.below(4) == 0).then(|| num(rng, wild)),
        lvl_restart: (rng.below(5) == 0).then(|| num(rng, wild)),
        ..ListLevelSpec::default()
    }
}

fn format(rng: &mut Rng, wild: bool) -> StyleFormat {
    let mut f = StyleFormat::default();
    let pick = |rng: &mut Rng| rng.below(3);
    if pick(rng) == 0 {
        f.bold = Ch::Val(rng.below(2) == 0);
    }
    if pick(rng) == 0 {
        f.sz_half_pt = Ch::Val(num(rng, wild) * 4.0);
    }
    if pick(rng) == 0 {
        f.font_ascii = if rng.below(4) == 0 { Ch::Null } else { Ch::Val("Arial".to_owned()) };
    }
    if pick(rng) == 0 {
        f.color = Ch::Val(["FF0000", "", "a\"b"][rng.below(3)].to_owned());
    }
    if pick(rng) == 0 {
        f.alignment = Ch::Val(["center", "left", "both"][rng.below(3)].to_owned());
    }
    if pick(rng) == 0 {
        f.indent_first_line_twips = Ch::Val(num(rng, wild) * 100.0 - 300.0);
    }
    if pick(rng) == 0 {
        f.line_twentieths = if rng.below(3) == 0 { Ch::Null } else { Ch::Val(num(rng, wild) * 30.0) };
    }
    f
}

/// One seeded F2 command. `wild`: NaN, infinities, huge numbers, hostile ids and names.
pub fn f2_cmd(rng: &mut Rng, wild: bool) -> Command {
    let id = |rng: &mut Rng| IDS[rng.below(IDS.len())].to_owned();
    let name = |rng: &mut Rng| NAMES[rng.below(NAMES.len())].to_owned();
    let idx = |rng: &mut Rng| (rng.below(6) != 0).then(|| if wild && rng.below(4) == 0 { rng.below(100) } else { rng.below(8) });
    match rng.below(34) {
        0 => Command::List(ListCmd::ToggleList { bullet: rng.below(2) == 0 }),
        1 | 2 => Command::List(ListCmd::ApplyListLibrary { bullet: rng.below(2) == 0, index: idx(rng) }),
        3 => Command::List(ListCmd::RemoveList),
        4 => Command::List(ListCmd::ApplyMultilevelLibrary { index: idx(rng) }),
        5 => Command::List(ListCmd::ApplyListLevels { levels: (0..rng.below(4)).map(|_| level(rng, wild)).collect() }),
        6 | 7 => Command::List(ListCmd::SetListLevel { ilvl: num(rng, wild) }),
        8 | 9 => Command::List(ListCmd::ChangeListLevel { delta: if rng.below(2) == 0 { 1 } else { -1 } }),
        10 => Command::List(ListCmd::RestartNumbering { start: (rng.below(2) == 0).then(|| num(rng, wild)) }),
        11 => Command::List(ListCmd::ContinueNumbering),
        12 => Command::List(ListCmd::SetNumberingValue { new_list: rng.below(2) == 0, value: (rng.below(2) == 0).then(|| num(rng, wild)) }),
        13 => Command::List(ListCmd::DefineListLevel {
            format: Box::new(ListLevelFormat {
                bullet: (rng.below(2) == 0).then(|| ["\u{2022}", "", "\"", "\u{f0b7}"][rng.below(4)].to_owned()),
                num_fmt: Some(["decimal", "", "x\"y"][rng.below(3)].to_owned()),
                lvl_text: (rng.below(2) == 0).then(|| "(%1)".to_owned()),
                start: (rng.below(2) == 0).then(|| num(rng, wild)),
                align: (rng.below(3) == 0).then(|| "right".to_owned()),
                font: (rng.below(2) == 0).then(|| "Symbol".to_owned()),
                bold: rng.below(2) == 0,
                italic: rng.below(2) == 0,
                size_pt: (rng.below(2) == 0).then(|| num(rng, wild)),
                color: (rng.below(2) == 0).then(|| "00FF00".to_owned()),
            }),
        }),
        14 => Command::List(ListCmd::DefineMultilevelList {
            levels: (0..rng.below(4)).map(|_| level(rng, wild)).collect(),
            forward: rng.below(2) == 0,
        }),
        15 => Command::List(ListCmd::CreateListStyle {
            name: name(rng),
            levels: (0..rng.below(3)).map(|_| level(rng, wild)).collect(),
            apply: rng.below(2) == 0,
        }),
        16 => Command::List(ListCmd::ApplyListStyle { style_id: id(rng) }),
        17 => Command::List(ListCmd::ModifyListStyle { style_id: id(rng), levels: (0..rng.below(3)).map(|_| level(rng, wild)).collect() }),
        18..=20 => Command::Style(StyleCmd::ApplyStyle { style_id: (rng.below(5) != 0).then(|| id(rng)) }),
        21 => Command::Style(StyleCmd::CreateStyle {
            spec: Box::new(CreateStyleSpec {
                name: name(rng),
                kind: (rng.below(3) != 0).then(|| ["linked", "paragraph", "character", "bogus"][rng.below(4)].to_owned()),
                based_on: (rng.below(3) == 0).then(|| id(rng)),
                next: (rng.below(3) == 0).then(|| id(rng)),
                quick: (rng.below(2) == 0).then(|| rng.below(2) == 0),
                auto_update: rng.below(2) == 0,
                format: (rng.below(2) == 0).then(|| format(rng, wild)),
                from_selection: (rng.below(3) == 0).then(|| rng.below(2) == 0),
                apply: (rng.below(3) == 0).then(|| rng.below(2) == 0),
            }),
        }),
        22 | 23 => Command::Style(StyleCmd::ModifyStyle {
            id: id(rng),
            spec: Box::new(ModifyStyleSpec {
                name: (rng.below(3) == 0).then(|| name(rng)),
                based_on: (rng.below(3) == 0).then(|| id(rng)),
                next: match rng.below(4) {
                    0 => Ch::Null,
                    1 => Ch::Val(id(rng)),
                    _ => Ch::Keep,
                },
                quick: (rng.below(3) == 0).then(|| rng.below(2) == 0),
                auto_update: (rng.below(3) == 0).then(|| rng.below(2) == 0),
                format: format(rng, wild),
            }),
        }),
        24 => Command::Style(StyleCmd::SetDefaultFont {
            font: Box::new(DefaultFont {
                font_family: (rng.below(2) == 0).then(|| "Arial".to_owned()),
                size_pt: (rng.below(2) == 0).then(|| num(rng, wild)),
                bold: (rng.below(2) == 0).then(|| rng.below(2) == 0),
                color: if rng.below(3) == 0 { Ch::Null } else { Ch::Keep },
                spacing_twips: (rng.below(2) == 0).then(|| num(rng, wild)),
                ..DefaultFont::default()
            }),
        }),
        25 => Command::Style(StyleCmd::UpdateStyleFromSelection { id: (rng.below(2) == 0).then(|| id(rng)) }),
        26 => Command::Style(StyleCmd::RenameStyle { id: id(rng), name: name(rng) }),
        27 => Command::Style(StyleCmd::DeleteStyle { id: id(rng) }),
        28 => Command::Style(StyleCmd::SetStyleInGallery { id: id(rng), on: rng.below(2) == 0 }),
        29 => Command::Style(StyleCmd::ClearParagraphStyle),
        30 => Command::Style(StyleCmd::ClearCharacterStyle),
        31 => Command::Style(StyleCmd::ClearParagraphFormatting),
        32 => Command::Style(StyleCmd::ClearCharacterFormatting),
        _ => Command::Text(TextCmd::Enter),
    }
}

/// A selection somewhere in the story.
pub fn some_selection(rng: &mut Rng, s: &Session) -> Command {
    let paras = s.paras().unwrap();
    let pos = |rng: &mut Rng| {
        let para = rng.below(paras.len());
        let len = paras.get(para).map_or(0, uo_docx::session::text::para_length);
        DocPosition::new(u32::try_from(para).unwrap(), u32::try_from(rng.below(len + 1)).unwrap())
    };
    let a = pos(rng);
    let f = if rng.below(2) == 0 { a } else { pos(rng) };
    Command::Text(TextCmd::SetSelection { anchor: a, focus: f })
}

pub fn typed(rng: &mut Rng) -> Command {
    Command::Text(TextCmd::Type { text: Utf16::from(["a", "Olá ", "x y"][rng.below(3)]) })
}

/// A blank document whose numbering and styles parts are replaced by `numbering` / `styles`.
pub fn doc_with_parts(numbering: Option<&str>, styles: Option<&str>) -> Vec<u8> {
    let mut pkg = uo_docx::read::blank::blank_docx_package(&uo_docx::read::blank::BlankDocxOptions::default());
    if let Some(n) = numbering {
        pkg.set_part_text("word/numbering.xml", n.to_owned());
        let rels = pkg.part_text("word/_rels/document.xml.rels").unwrap().map(|t| t.into_owned()).unwrap_or_default();
        if !rels.contains("numbering") {
            pkg.set_part_text(
                "word/_rels/document.xml.rels",
                rels.replace("</Relationships>", "<Relationship Id=\"rIdN\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/></Relationships>"),
            );
        }
    }
    if let Some(s) = styles {
        pkg.set_part_text("word/styles.xml", s.to_owned());
    }
    pkg.save(&uo_opc::package::SaveOptions::default()).unwrap()
}
