//! Parity of D2b-F10 (fields, TOC, table of figures, captions, cross-references, Manual Hyphenation, date
//! and time, document info, alignment tab) against the frozen `<universal-write>` element in Chromium (G17):
//! replays `tests/parity/d2b-f10-session.capture`, written by `js/capture-d2b-f10-session.mjs`.
//!
//! The layout oracle is a REPLAY ([`ReplayOracle`]): what the TS layout answered inside the step
//! (`i-fv`: the values `refreshFieldCache` handed to `setFieldResult`; `i-pg`: `pageLabelOf`; `i-mh`: the
//! word `nextManualHyphenation` got) is what the session's oracle answers, and the test asserts HOW the
//! session asked: `fvs` (the number of `field_values` calls and the scope of each: every field, or the
//! same set of field runs the TS passed as `only`), `pgc`/`pgv` (the page labels asked once, and the request
//! covers every paragraph the TS asked `pageLabelOf` about), `mhc` (the forced-hyphenation layout asked when
//! the TS got a word).
//!
//! Facets per step: the D2a ones (`paras`, `sel`, `cursel`, `anc`, `ev`, `flags`, `docxml`) and F10's: `res`
//! (what the op returned), `cap` (labels, numbering, next number, AutoCaption), `mh` (the Manual Hyphenation
//! state), `xri` (every cross-reference item list), `toco` (the TOC and table-of-figures options, the level),
//! `dtf` and `stylexml` (every 8th step), `rs` (`refSeed.n`).
//!
//! A step where the TS threw ends the document's script in the capture; the replay stops there too.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node --experimental-transform-types
//! core/crates/uo-docx/js/capture-d2b-f10-session.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f10_support;
#[allow(unused_imports)]
use crate::support;

use std::collections::HashSet;
use std::path::Path;

use d2b_f10_support::hostile::{TYPES, kinds};
use d2b_f10_support::replay::{ReplayOracle, StepScript};
use uo_docx::canon::CanonWriter;
use uo_docx::js::JsValue;
use uo_docx::session::ops::references::pure::{CaptionChapter, TocStyle};
use uo_docx::session::ops::references::{
    AutoCaption, AutoCaptionCfg, CaptionFormat, CaptionNumbering, CaptionOptions, CaptionPosition, CaptionSettings, ChapterSeparator,
    CrossRefOptions, CrossRefType, DocInfoKind, F10Result, FieldKind, FieldScopeSel, ReferenceCmd, TableOfContentsOptions,
    TableOfFiguresOptions, TocFormat, TocMode, TocTemplateId, TofFormat,
};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::styles::UiLang;
use uo_docx::session::ops::tables::{TableCmd, TableLayoutIn};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{ChangeSet, ClipboardRing, Command, DocPosition, Event, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;
use uo_docx::write::sig::SigMode;
use uo_testkit::{Capture, Digest, Replay, Stream, repo_root};

// ── the tables the capture indexes (js/capture-d2b-f10-session.mjs) ───────────────────────────────

const DTP: [&str; 11] = [
    "dd/MM/yyyy",
    "M/d/yyyy",
    "dddd, MMMM d, yyyy",
    "HH:mm",
    "h:mm AM/PM",
    "yyyy-MM-dd",
    "d 'de' MMMM 'de' yyyy",
    "",
    "MMM d",
    "dddd",
    "dd/MM/yyyy HH:mm",
];
const LANGS: [Option<UiLang>; 4] = [Some(UiLang::En), Some(UiLang::Pt), Some(UiLang::Es), None];
const INFOS: [DocInfoKind; 4] = [DocInfoKind::Author, DocInfoKind::Title, DocInfoKind::FileName, DocInfoKind::FilePath];
const ALIGN: [&str; 3] = ["left", "center", "right"];
const RELTO: [&str; 2] = ["margin", "indent"];
const LEADER: [&str; 5] = ["none", "dot", "hyphen", "underscore", "middleDot"];
const MKIDS: [Option<&str>; 4] = [None, Some("C"), Some("x"), Some("a b")];
const MKLVL: [Option<f64>; 6] = [None, Some(1.0), Some(2.0), Some(0.0), Some(9.0), Some(12.0)];
const TLVL: [f64; 6] = [0.0, 1.0, 2.0, 3.0, 9.0, 12.0];
const TPL: [TocTemplateId; 3] = [TocTemplateId::Auto1, TocTemplateId::Auto2, TocTemplateId::Manual];

/// `NUMS[i]`: index 0 is `undefined`.
fn nums(i: usize) -> Option<CaptionNumbering> {
    let ch = |level: f64, separator| Some(CaptionChapter { level, separator });
    Some(match i {
        0 => return None,
        1 => CaptionNumbering { format: CaptionFormat::Arabic, chapter: None },
        2 => CaptionNumbering { format: CaptionFormat::RomanUpper, chapter: ch(1.0, ChapterSeparator::Period) },
        3 => CaptionNumbering { format: CaptionFormat::AlphabeticLower, chapter: None },
        4 => CaptionNumbering { format: CaptionFormat::AlphabeticUpper, chapter: ch(2.0, ChapterSeparator::Hyphen) },
        5 => CaptionNumbering { format: CaptionFormat::RomanLower, chapter: ch(1.0, ChapterSeparator::EmDash) },
        _ => panic!("nums {i}"),
    })
}

fn dialog(i: usize) -> TableOfContentsOptions {
    let mut d = TableOfContentsOptions::default();
    match i {
        0 => {}
        1 => d.page_numbers = Some(false),
        2 => {
            d.right_align = Some(false);
            d.leader = Some("hyphen".into());
        }
        3 => {
            d.hyperlinks = Some(false);
            d.levels = Some(2.0);
        }
        4 => {
            d.format = Some(TocFormat::Classic);
            d.levels = Some(4.0);
        }
        5 => {
            d.use_styles = Some(false);
            d.fields = Some(true);
        }
        6 => {
            d.outline_levels = Some(false);
            d.leader = Some("none".into());
            d.format = Some(TocFormat::Modern);
        }
        7 => {
            d.styles = Some(vec![TocStyle { style: "Quote".into(), level: 2.0 }]);
            d.levels = Some(1.0);
        }
        8 => {
            d.fields = Some(true);
            d.field_id = Some("x".into());
            d.format = Some(TocFormat::Fancy);
        }
        9 => {
            d.leader = Some("underscore".into());
            d.right_align = Some(true);
            d.format = Some(TocFormat::Simple);
            d.levels = Some(9.0);
        }
        _ => panic!("dialog {i}"),
    }
    d
}

fn tof(label: String, i: usize) -> TableOfFiguresOptions {
    let mut o = TableOfFiguresOptions { label, ..TableOfFiguresOptions::default() };
    match i {
        0 => {}
        1 => o.include_label = Some(false),
        2 => o.page_numbers = Some(false),
        3 => {
            o.right_align = Some(false);
            o.leader = Some("hyphen".into());
        }
        4 => {
            o.hyperlinks = Some(true);
            o.format = Some(TofFormat::Classic);
        }
        5 => {
            o.leader = Some("none".into());
            o.format = Some(TofFormat::Centered);
        }
        6 => {
            o.format = Some(TofFormat::Formal);
            o.include_label = Some(false);
            o.hyperlinks = Some(false);
        }
        _ => panic!("tof {i}"),
    }
    o
}

fn cfg(i: usize) -> CaptionSettings {
    let mut c = CaptionSettings::default();
    match i {
        0 => {}
        1 => {
            c.labels = vec!["Gr\u{e1}fico".into(), "Chart".into()];
            c.numbering.insert("Gr\u{e1}fico".into(), CaptionNumbering { format: CaptionFormat::RomanLower, chapter: None });
            c.auto = Some(AutoCaptionCfg { table: Some(AutoCaption { label: "Table".into(), position: CaptionPosition::Above }) });
        }
        2 => {
            c.labels = vec!["Chart".into()];
            c.numbering.insert(
                "Chart".into(),
                CaptionNumbering {
                    format: CaptionFormat::AlphabeticUpper,
                    chapter: Some(CaptionChapter { level: 1.0, separator: ChapterSeparator::Colon }),
                },
            );
        }
        _ => panic!("cfg {i}"),
    }
    c
}

/// Known divergences of the capture from the session: from `step` on, the document's facets are accepted by name
/// (each is a gate item in `F10.md`).
const DIVERGENCES: [(&str, usize, &str); 1] = [(
    "fsep-top716.docx",
    30,
    "F10-A3: `removeTableOfContents` leaves the TS table anchor at beforePara -9 (`shiftAnchors` with a negative count past 0); the session's anchors are unsigned and stop at 0",
)];

fn ck(replay: &mut Replay<'_>, diverged: Option<&str>, key: &str, facet: &str, actual: &str) {
    match diverged {
        Some(reason) => {
            replay.accept(key, facet, actual, reason);
        }
        None => {
            replay.check(key, facet, actual);
        }
    }
}

fn units(hex: &str) -> Vec<u16> {
    hex.split('.').filter(|h| !h.is_empty()).map(|h| u16::from_str_radix(h, 16).unwrap()).collect()
}

fn hx(u: &[u16]) -> String {
    u.iter().map(|c| format!("{c:x}")).collect::<Vec<_>>().join(".")
}

fn hs(s: &str) -> String {
    hx(&s.encode_utf16().collect::<Vec<_>>())
}

fn text16(h: &str) -> String {
    String::from_utf16_lossy(&units(h))
}

fn sha(t: &str) -> String {
    Digest::of(t.as_bytes()).short()
}

fn js(n: f64) -> String {
    uo_jsnum::format::to_js_string(n)
}

// ── the D2a facets (as the F9 replay) ───────────────────────────────────────────────────────────

fn hyphenation_registry() -> std::sync::Arc<uo_fonts::hyphen::HyphenationRegistry> {
    // the shipped patterns of the three languages the element loads (the data files, parsed here so that the
    // test needs no feature of uo-fonts)
    let mut reg = uo_fonts::hyphen::HyphenationRegistry::new();
    for text in [
        include_str!("../../uo-fonts/data/hyphen/pt.txt"),
        include_str!("../../uo-fonts/data/hyphen/en-us.txt"),
        include_str!("../../uo-fonts/data/hyphen/es.txt"),
    ] {
        reg.register(&uo_fonts::hyphen::parse_patterns(text).unwrap());
    }
    std::sync::Arc::new(reg)
}

/// A canonical sink over a stream both the writer and the paragraph tags write to.
struct Shared<'a>(&'a std::cell::RefCell<Stream>);

impl uo_docx::canon::CanonSink for Shared<'_> {
    fn tag(&mut self, c: u8) {
        self.0.borrow_mut().tag(c);
    }
    fn u32(&mut self, v: u32) {
        self.0.borrow_mut().u32(v);
    }
    fn f64(&mut self, v: f64) {
        self.0.borrow_mut().f64(v);
    }
    fn bool(&mut self, v: bool) {
        self.0.borrow_mut().bool(v);
    }
    fn bytes(&mut self, b: &[u8]) {
        self.0.borrow_mut().bytes(b);
    }
    fn digest_of(&mut self, b: &[u8]) {
        self.0.borrow_mut().digest_of(b);
    }
}

/// The digest of the paragraphs as `digest(paras)` of the capture: ONE writer for all of them, so that the
/// relabelled sdt keys count across paragraphs as the TS `Canon` does.
fn paras_digest(s: &Session) -> String {
    let doc = s.document();
    let mut paras = doc.body.paras.clone();
    let mut ids = doc.ids.clone();
    uo_docx::session::normalize::normalize(&mut paras, &mut ids);
    let stream = std::cell::RefCell::new(Stream::new());
    let mut sink = Shared(&stream);
    let mut w = CanonWriter::new(&mut sink).with_media(&doc.media);
    for (i, p) in paras.iter().enumerate() {
        stream.borrow_mut().tag(b'E').u32(u32::try_from(i).unwrap());
        p.canon(&mut w);
    }
    drop(w);
    stream.into_inner().finish().short()
}

fn pos_json(p: DocPosition) -> String {
    format!("{{\"para\":{},\"offset\":{}}}", p.para, p.offset)
}

fn sel_json(a: DocPosition, f: DocPosition) -> String {
    format!("{{\"anchor\":{},\"focus\":{}}}", pos_json(a), pos_json(f))
}

fn anc_json(s: &Session) -> String {
    let t: Vec<String> = s.document().body.tables.iter().map(|a| a.before_para.to_string()).collect();
    let b: Vec<String> = s.bookmark_paras().iter().map(|(k, v)| format!("[{},{v}]", uo_jsnum::json::string(k))).collect();
    format!("{{\"t\":[{}],\"b\":[{}]}}", t.join(","), b.join(","))
}

fn ev_text(cs: &ChangeSet) -> String {
    let e: Vec<String> = cs
        .events
        .iter()
        .map(|e| match e {
            Event::Change => "uw-change:".to_owned(),
            Event::DirtyChange { dirty } => format!("uw-dirty-change:{dirty}"),
            Event::Input { .. } => "uw-input:".to_owned(),
            Event::TrackChanges { on } => format!("uw-track-changes:{on}"),
            Event::CellMode { active } => format!("uw-cellmode:{active}"),
            Event::NoteMode { active, .. } => format!("uw-notemode:{active}"),
            Event::HfMode { active } => format!("uw-hfmode:{active}"),
            Event::TbMode { active } => format!("uw-tbmode:{active}"),
            Event::Protected { active, .. } => format!("uw-protected:{active}"),
            Event::Blocked { .. } => "uw-blocked:".to_owned(),
            Event::StylesChange => "uw-styles-change:".to_owned(),
            Event::CaptionSettings => "uw-caption-settings:".to_owned(),
            other => format!("{other:?}"),
        })
        .collect();
    if e.is_empty() { "-".to_owned() } else { e.join(" ") }
}

fn flags(s: &Session) -> String {
    format!("{}{}{}:{}", u8::from(s.is_dirty()), u8::from(s.can_undo()), u8::from(s.can_redo()), s.edits_since_load())
}

fn part_digest(pkg: &uo_opc::Package, name: &str) -> String {
    pkg.part_text(name).unwrap().map_or_else(|| "-".to_owned(), |t| sha(&t))
}

fn saved(s: &mut Session) -> Option<uo_opc::Package> {
    let bytes = s.save_docx().ok()?;
    Some(uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap())
}

// ── the F10 facets ──────────────────────────────────────────────────────────────────────────────

fn chap_name(s: ChapterSeparator) -> &'static str {
    match s {
        ChapterSeparator::Hyphen => "hyphen",
        ChapterSeparator::Period => "period",
        ChapterSeparator::Colon => "colon",
        ChapterSeparator::EmDash => "emDash",
        ChapterSeparator::EnDash => "enDash",
    }
}

fn cap_text(s: &Session) -> String {
    let labels = s.caption_labels();
    let l: Vec<String> = labels
        .iter()
        .map(|l| {
            let n = s.caption_numbering(&l.name);
            let ch = n.chapter.map_or(String::new(), |c| format!(".{}.{}", js(c.level), chap_name(c.separator)));
            format!(
                "{}~{}{}~{}{}~{}",
                hs(&l.name),
                u8::from(l.built_in),
                if l.position == CaptionPosition::Above { 'a' } else { 'b' },
                n.format.as_str(),
                ch,
                hx(s.next_caption_number(&l.name).units())
            )
        })
        .collect();
    let auto = match s.auto_caption() {
        Some(AutoCaptionCfg { table: Some(t) }) => {
            format!("{}{}", hs(&t.label), if t.position == CaptionPosition::Above { 'a' } else { 'b' })
        }
        Some(_) => "e".to_owned(),
        None => "-".to_owned(),
    };
    format!("{}|{auto}", l.join(";"))
}

fn mh_text(s: &Session) -> String {
    match s.manual_hyphenation_state() {
        None => "-".to_owned(),
        Some(m) => {
            let to = m.to.unwrap_or(m.from);
            let cur = m.cur.map_or_else(
                || "-".to_owned(),
                |c| {
                    let pts: Vec<String> = c.points.iter().map(u32::to_string).collect();
                    format!("{}.{}.{}.{}.{}", c.para, c.start, c.end, pts.join(","), c.limit)
                },
            );
            format!("{}.{}-{}.{}|{cur}|{}", m.from.para, m.from.offset, to.para, to.offset, m.skipped)
        }
    }
}

fn xri_text(s: &Session) -> String {
    let row = |name: String, items: Vec<uo_docx::session::ops::references::CrossRefItem>| {
        format!("{name}={}", items.iter().map(|i| format!("{}~{}", i.id, hx(i.text.units()))).collect::<Vec<_>>().join(";"))
    };
    let mut rows = Vec::new();
    for (name, t) in [
        ("numbered", CrossRefType::Numbered),
        ("heading", CrossRefType::Heading),
        ("bookmark", CrossRefType::Bookmark),
        ("footnote", CrossRefType::Footnote),
        ("endnote", CrossRefType::Endnote),
    ] {
        rows.push(row(name.to_owned(), s.cross_reference_items(t, None)));
    }
    for l in s.caption_labels() {
        rows.push(row(format!("caption:{}", hs(&l.name)), s.cross_reference_items(CrossRefType::Caption, Some(&l.name))));
    }
    rows.join("\n")
}

fn f1b(v: Option<bool>) -> String {
    v.map_or_else(|| "-".to_owned(), |b| b.to_string())
}

fn f1s(v: Option<&str>) -> String {
    v.map_or_else(|| "-".to_owned(), hs)
}

fn toc_fmt_name(f: TocFormat) -> &'static str {
    match f {
        TocFormat::Template => "template",
        TocFormat::Classic => "classic",
        TocFormat::Distinctive => "distinctive",
        TocFormat::Fancy => "fancy",
        TocFormat::Modern => "modern",
        TocFormat::Formal => "formal",
        TocFormat::Simple => "simple",
    }
}

fn toco_text(s: &Session) -> String {
    let to = s.table_of_contents_options().map_or_else(
        || "null".to_owned(),
        |o| {
            [
                f1b(o.page_numbers),
                f1b(o.right_align),
                f1s(o.leader.as_deref()),
                f1b(o.hyperlinks),
                f1s(o.format.map(toc_fmt_name)),
                o.levels.map_or_else(|| "-".to_owned(), js),
                f1b(o.use_styles),
                o.styles.as_ref().map_or_else(
                    || "-".to_owned(),
                    |st| st.iter().map(|x| format!("{};{}", hs(&x.style), js(x.level))).collect::<Vec<_>>().join("/"),
                ),
                f1b(o.outline_levels),
                f1b(o.fields),
                f1s(o.field_id.as_deref()),
            ]
            .join(",")
        },
    );
    let tf = s.table_of_figures_options().map_or_else(
        || "null".to_owned(),
        |o| {
            [hs(&o.label), f1b(o.include_label), f1b(o.page_numbers), f1b(o.right_align), f1b(o.hyperlinks), f1s(o.leader.as_deref())]
                .join(",")
        },
    );
    format!("{}{}|{}|{to}|{tf}", u8::from(s.has_table_of_contents()), u8::from(s.in_table_of_contents()), js(s.toc_level()))
}

fn dtf_text(s: &Session) -> String {
    let one = |l: Option<UiLang>| {
        s.date_time_formats(l).iter().map(|(p, x)| format!("{}={}", hs(p), hx(x.units()))).collect::<Vec<_>>().join(";")
    };
    [Some(UiLang::En), Some(UiLang::Pt), Some(UiLang::Es), None].map(one).join("|")
}

/// What the op returned, as the TS harness writes it.
fn res_of(s: &Session, code: &str) -> String {
    let op = code.split(':').next().unwrap();
    let r = s.references_result();
    match op {
        "uf" => match r {
            Some(F10Result::Count(n)) => n.to_string(),
            other => format!("{other:?}"),
        },
        "xr" | "ic" | "tf" | "ut2" | "tc" | "tt" | "ut" | "rt" | "mk" | "cl" | "dl" => match r {
            Some(F10Result::Bool(b)) => b.to_string(),
            other => format!("{other:?}"),
        },
        "mn" => match r {
            Some(F10Result::Hyphenation(Some(m))) => {
                let pts: Vec<String> = m.points.iter().map(u32::to_string).collect();
                format!("{}|{}|{}|{}|{}", hx(m.word.units()), m.lang, pts.join(","), m.suggested, m.limit)
            }
            Some(F10Result::Hyphenation(None)) => "null".to_owned(),
            other => format!("{other:?}"),
        },
        _ => "-".to_owned(),
    }
}

// ── the steps ───────────────────────────────────────────────────────────────────────────────────

fn command(code: &str) -> Option<Command> {
    let f: Vec<&str> = code.split(':').collect();
    let nn = |i: usize| f[i].parse::<usize>().unwrap();
    let r = Command::Reference;
    Some(match f[0] {
        "ty" => Command::Text(TextCmd::Type { text: Utf16::from_units(units(f[1])) }),
        "en" => Command::Text(TextCmd::Enter),
        "bs" => Command::Text(TextCmd::Backspace),
        "de" => Command::Text(TextCmd::Delete),
        "se" => Command::Text(TextCmd::SetSelection {
            anchor: DocPosition::new(nn(1) as u32, nn(2) as u32),
            focus: DocPosition::new(nn(3) as u32, nn(4) as u32),
        }),
        "tk" => Command::Settings(SettingsCmd::SetTracking { on: f[1] == "1" }),
        "if" => r(ReferenceCmd::InsertField { kind: [FieldKind::Page, FieldKind::NumPages, FieldKind::Date][nn(1)] }),
        "dt" => r(ReferenceCmd::InsertDateTime { pattern: DTP[nn(1)].encode_utf16().collect(), update: f[2] == "1", lang: LANGS[nn(3)] }),
        "di" => r(ReferenceCmd::InsertDocumentInfo { kind: INFOS[nn(1)], file_name: "file.docx".into() }),
        "at" => r(ReferenceCmd::InsertAlignmentTab {
            alignment: ALIGN[nn(1)].into(),
            relative_to: RELTO[nn(2)].into(),
            leader: LEADER[nn(3)].into(),
        }),
        "uf" => r(ReferenceCmd::UpdateFields {
            scope: [FieldScopeSel::Selection, FieldScopeSel::All][nn(1)],
            toc: [TocMode::Pages, TocMode::All][nn(2)],
        }),
        "xr" => {
            let fl = nn(5);
            r(ReferenceCmd::InsertCrossReference(CrossRefOptions {
                ty: TYPES[nn(1)],
                label: (!f[2].is_empty()).then(|| text16(f[2])),
                item: text16(f[3]),
                kind: kinds()[nn(4)],
                hyperlink: (fl & 1 != 0).then_some(false),
                position: (fl & 2 != 0).then_some(true),
                separator: None,
            }))
        }
        "ic" => r(ReferenceCmd::InsertCaption(CaptionOptions {
            label: text16(f[1]),
            title: (!f[2].is_empty()).then(|| text16(f[2])),
            position: match nn(3) {
                0 => None,
                1 => Some(CaptionPosition::Above),
                _ => Some(CaptionPosition::Below),
            },
            exclude_label: (nn(4) != 0).then_some(true),
            numbering: nums(nn(5)),
        })),
        "tf" => r(ReferenceCmd::InsertTableOfFigures(tof(text16(f[1]), nn(2)))),
        "ut2" => r(ReferenceCmd::UpdateTableOfFigures),
        "tc" => r(ReferenceCmd::InsertTableOfContents { opts: dialog(nn(1)), replace: [None, Some(true), Some(false)][nn(2)] }),
        "tt" => r(ReferenceCmd::InsertTocTemplate { id: TPL[nn(1)] }),
        "ut" => r(ReferenceCmd::UpdateToc { mode: [TocMode::Pages, TocMode::All][nn(1)] }),
        "rt" => r(ReferenceCmd::RemoveTableOfContents),
        "tl" => r(ReferenceCmd::SetTocLevel { level: TLVL[nn(1)] }),
        "mk" => r(ReferenceCmd::MarkTocEntry { text: text16(f[1]), identifier: MKIDS[nn(2)].map(str::to_owned), level: MKLVL[nn(3)] }),
        "cl" => r(ReferenceCmd::AddCaptionLabel { name: text16(f[1]) }),
        "dl" => r(ReferenceCmd::DeleteCaptionLabel { name: text16(f[1]) }),
        "cn" => r(ReferenceCmd::SetCaptionNumbering { label: text16(f[1]), numbering: nums(nn(2)).unwrap_or_default() }),
        "ac" => r(ReferenceCmd::SetAutoCaption {
            auto: (!f[1].is_empty()).then(|| (text16(f[1]), [CaptionPosition::Above, CaptionPosition::Below][nn(2)])),
        }),
        "cs" => r(ReferenceCmd::SetCaptionSettings(cfg(nn(1)))),
        "it" => Command::Table(TableCmd::InsertTable {
            rows: nn(1) as f64,
            cols: nn(2) as f64,
            autofit: None,
            col_width_twips: None,
            layout: TableLayoutIn::default(),
        }),
        "mn" => r(ReferenceCmd::NextManualHyphenation),
        "ma" => r(ReferenceCmd::ApplyManualHyphenation { letters: nn(1) as f64 }),
        "ms" => r(ReferenceCmd::SkipManualHyphenation),
        "me" => r(ReferenceCmd::EndManualHyphenation),
        "un" | "re" => return None,
        _ => panic!("step {code}"),
    })
}

fn step(s: &mut Session, o: &mut ReplayOracle, code: &str) -> ChangeSet {
    let mut clip = ClipboardRing::default();
    match command(code) {
        None if code == "un" => s.undo(&mut o.cx(&mut clip)),
        None => s.redo(&mut o.cx(&mut clip)),
        Some(c) => match s.exec(c, &mut o.cx(&mut clip)) {
            Ok(Outcome::Done(cs)) => cs,
            Ok(Outcome::NeedsJobs(_)) => panic!("no jobs in F10 scripts"),
            Err(e) => panic!("{code}: {e}"),
        },
    }
}

#[test]
fn d2b_f10_session_matches_the_element() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let path = std::env::var("UO_F10_CAPTURE").map_or_else(|_| dir.join("d2b-f10-session.capture"), std::path::PathBuf::from);
    let cap = Capture::read(path).unwrap();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let steps: usize = cap.meta("steps").unwrap().parse().unwrap();
    let mut replay = Replay::new(&cap);
    let mut docs = 0;
    let (mut n_fv, mut n_pg, mut n_mh, mut n_all, mut n_unk) = (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut ops = std::collections::BTreeMap::<String, usize>::new();
    for rec in cap.records().iter().filter(|r| r.key.ends_with("#0")) {
        let rel = rec.key.trim_end_matches("#0");
        let bytes = std::fs::read(root.join(rel)).unwrap();
        // the element draws one `Math.random()` at construction (`refSeed`): the session's source starts one
        // value on, and the seed is the one the capture recorded
        let mut env = FixedEnv::default();
        uo_docx::session::Env::random_u32(&mut env);
        let mut s = Session::open(bytes, OpenOptions { sig_mode: SigMode::Ts, ..OpenOptions::default() }, Box::new(env)).unwrap();
        s.set_ui_lang(UiLang::Pt);
        s.set_ref_seed(cap.get(&rec.key, "i-seed").unwrap().parse().unwrap());
        let diverges = DIVERGENCES.iter().find(|(d, _, _)| rel.ends_with(d)).map(|(_, s, r)| (*s, *r));
        let mut o = ReplayOracle::new();
        let mut clip = ClipboardRing::default();
        s.exec(Command::Settings(SettingsCmd::SetAuthor { name: "A".into() }), &mut o.cx(&mut clip)).unwrap();
        // the hyphenation patterns of the element (`ENGINE_LAYOUT_OPTS.hyphenationLanguages`: pt-BR, en, es)
        s.set_hyphenation_registry(Some(hyphenation_registry()));
        for j in 0..=steps {
            let key = format!("{rel}#{j}");
            let Some(code) = cap.get(&key, "op") else { break };
            replay.check(&key, "op", code);
            if cap.get(&key, "thrown").is_some() {
                replay.skip(&key, "thrown");
                break;
            }
            let mut ev = None;
            let mut res = "-".to_owned();
            let mut had_sel = false;
            if j > 0 {
                let sel0 = s.selection();
                had_sel = sel0.anchor != sel0.focus;
                o.begin(StepScript::parse(cap.get(&key, "i-fv"), cap.get(&key, "i-pg"), cap.get(&key, "i-mh")));
                let cs = step(&mut s, &mut o, code);
                ev = Some(ev_text(&cs));
                res = res_of(&s, code);
                *ops.entry(code.split(':').next().unwrap().to_owned()).or_default() += 1;
            }
            let unk: usize = cap.get(&key, "unk").map_or(0, |u| u.split_once('.').unwrap().0.parse().unwrap());
            if unk > 0 {
                n_unk += 1;
            }
            if std::env::var("UO_F10_DUMP").is_ok_and(|t| key.contains(&t)) {
                println!("RUSTDUMP {key} {code}");
                for (i, p) in s.document().body.paras.iter().enumerate() {
                    let runs: Vec<String> = p
                        .runs
                        .iter()
                        .map(|r| {
                            let f = r.ext().and_then(|e| e.field.as_deref()).map_or(String::new(), |f| format!("[{f}]"));
                            let b = if r.ext().is_some_and(|e| e.field_begin.is_some()) { "{B}" } else { "" };
                            let en = if r.ext().is_some_and(|e| e.field_end == Some(true)) { "{E}" } else { "" };
                            format!("{}{f}{b}{en}", uo_jsnum::json::string(&r.text.to_string_lossy()))
                        })
                        .collect();
                    let line = runs.join(" ");
                    println!("DUMP {i} s={} {}", p.props.style_id.as_deref().unwrap_or(""), line.chars().take(220).collect::<String>());
                }
                println!("DUMP anchors {:?}", s.document().body.tables.iter().map(|a| a.before_para).collect::<Vec<_>>());
            }
            // the oracle use
            if j > 0 {
                if std::env::var("UO_F10_TRACE").is_ok_and(|t| key.contains(&t)) {
                    println!(
                        "ORACLE {key} {code}: scopes {:?}, pages asked {} {:?}, ts i-pg {:?}",
                        o.fv_scopes,
                        o.pg_calls,
                        o.pg_requested,
                        cap.get(&key, "i-pg")
                    );
                }
                n_fv += o.fv_scopes.len();
                n_all += o.fv_scopes.iter().filter(|d| *d == "A").count();
                n_pg += o.pg_calls;
                n_mh += o.mh_calls;
                let want_pg = cap.get(&key, "pgv").unwrap();
                let asked: HashSet<u32> = cap
                    .get(&key, "i-pg")
                    .filter(|x| *x != "-")
                    .map(|x| x.split(',').map(|kv| kv.split_once(':').unwrap().0.parse().unwrap()).collect())
                    .unwrap_or_default();
                let missing: Vec<u32> = asked.iter().copied().filter(|a| !o.pg_requested.contains(a)).collect();
                let pgv = if missing.is_empty() { format!("ok:{}", asked.len()) } else { format!("missing:{missing:?} of {want_pg}") };
                // a TS that asked the page labels: ONE question; a TS that did not: none
                let pgc = if o.pg_calls == 0 {
                    "0"
                } else if o.pg_calls == 1 {
                    "1"
                } else {
                    "many"
                };
                let div = diverges.filter(|(s, _)| j >= *s).map(|(_, r)| r);
                ck(&mut replay, div, &key, "fvs", &o.fvs());
                if pgc == "1"
                    && cap.get(&key, "pgc") == Some("0")
                    && had_sel
                    && ["tt", "tc", "tf"].contains(&code.split(':').next().unwrap())
                {
                    replay.accept(
                        &key,
                        "pgc",
                        pgc,
                        "F10-A4: with a selection the command deletes it before it reads the sources; the session asks the page labels without knowing whether a source is left",
                    );
                } else {
                    ck(&mut replay, div, &key, "pgc", pgc);
                }
                ck(&mut replay, div, &key, "pgv", &pgv);
                let mhc = if cap.get(&key, "mhc") == Some("1") {
                    if o.mh_calls == 1 { "1".to_owned() } else { format!("calls={}", o.mh_calls) }
                } else if o.mh_calls <= 1 {
                    "x".to_owned()
                } else {
                    format!("calls={}", o.mh_calls)
                };
                ck(&mut replay, div, &key, "mhc", &mhc);
                assert_eq!(o.fv_missing, 0, "{key} {code}: a value of the TS names a run the model does not have");
                for f in ["i-fv", "i-pg", "i-mh", "unk"] {
                    replay.skip(&key, f);
                }
            }
            if j == 0 {
                replay.skip(&key, "i-seed");
            }
            let sel = s.selection();
            let body_sel = uo_docx::session::ops::context::saved_selection(s.context()).unwrap_or(sel);
            let r = if res.len() > 200 { sha(&res) } else { res };
            let cap_t = cap_text(&s);
            let cap_t = if cap_t.len() > 200 { sha(&cap_t) } else { cap_t };
            let toco = toco_text(&s);
            let toco = if toco.len() > 200 { sha(&toco) } else { toco };
            if std::env::var("UO_F10_TRACE").is_ok_and(|t| key.contains(&t)) {
                println!(
                    "RUST {key} {code}
{}
",
                    xri_text(&s)
                );
            }
            let mut facets: Vec<(&str, String)> = vec![
                ("paras", paras_digest(&s)),
                ("sel", sel_json(body_sel.anchor, body_sel.focus)),
                ("cursel", sel_json(sel.anchor, sel.focus)),
                ("anc", anc_json(&s)),
                ("flags", flags(&s)),
                ("res", r),
                ("cap", cap_t),
                ("mh", mh_text(&s)),
                ("xri", sha(&xri_text(&s))),
                ("toco", toco),
                ("rs", s.ref_seed().map_or_else(|| "-".to_owned(), js)),
            ];
            match ev {
                Some(ev) => facets.push(("ev", ev)),
                None => replay.skip(&key, "ev"),
            }
            if cap.get(&key, "dtf").is_some() {
                facets.push(("dtf", sha(&dtf_text(&s))));
            }
            if cap.get(&key, "docxml").is_some() {
                match saved(&mut s) {
                    Some(pkg) => {
                        facets.push(("docxml", part_digest(&pkg, "word/document.xml")));
                        facets.push(("stylexml", part_digest(&pkg, "word/styles.xml")));
                    }
                    None => {
                        facets.push(("docxml", "err".to_owned()));
                        facets.push(("stylexml", "err".to_owned()));
                    }
                }
            }
            for (f, actual) in facets {
                ck(&mut replay, diverges.filter(|(s, _)| j >= *s).map(|(_, r)| r), &key, f, &actual);
            }
        }
        docs += 1;
    }
    let report = replay.finish();
    println!(
        "d2b-f10-session: {docs} documents, {} | field_values calls {n_fv} (scope All {n_all}), shown_page_numbers calls {n_pg}, next_hyphenation calls {n_mh}, steps with unnamed fields {n_unk} | ops {ops:?}",
        report.summary()
    );
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_D2A_ALL").map_or(40, |_| 100_000)));
    }
    report.assert_clean();
}
