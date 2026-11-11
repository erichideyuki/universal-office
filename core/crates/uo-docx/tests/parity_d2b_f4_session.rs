//! Parity of D2b-F4 (sections, page setup, headers/footers, page colour, text layout options) against
//! the frozen `<universal-write>` element in Chromium (G17): replays
//! `tests/parity/d2b-f4-session.capture`, written by `js/capture-d2b-f4-session.mjs`. Seeded scripts
//! over corpus documents (those with several sections or header/footer references first) mix typing
//! with page setup, section/page/column breaks, blank pages, columns, line numbers, section start
//! and vertical alignment, headers/footers in and out of edit (different first page, odd/even,
//! distances, link to previous, remove, page numbers), the default tab, hyphenation, the page colour
//! and fill effects, and undo/redo. Facets per step: the D2a ones (`paras`, `sel`, `anc`, `ev`,
//! `flags`, `docxml`) and F4's: `sec` (the effective sections), `body` (the body `sectPr` and
//! references), `st` (even/odd, default tab, hyphenation, background), `hfp` (the header/footer
//! parts), `ctx` (the part in edit), `parts` (the saved settings, header/footer parts, their
//! relationships, the content types and the styles).
//!
//! The layout's answers the commands take as arguments are in the capture: `hc` (`hfContext()`),
//! `hp` (the page's section and variant), `hs` (the section of the page in edit), `reenter` (the
//! part in edit after the step), `cr` (the part `enterHeaderFooter` created).
//!
//! Accepted by name: the events of the install (step 0) and the list rules of D2b-F2 (D2a-T4). The
//! part the header/footer edit returns to after an undo or redo (D2a-C2) is the layout's answer in
//! the capture (`reenter`, `cr`), passed to `undo_reentering`/`redo_reentering`.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f4-session.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::support;

use std::path::Path;

use d2a_support::Cx;
use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::ids::HfPartId;
use uo_docx::js::{JsNumber, JsValue};
use uo_docx::read::watermark::WatermarkInfo;
use uo_docx::session::ops::context::ContextCmd;
use uo_docx::session::ops::format::{ColorArg, ThemeColorRef};
use uo_docx::session::ops::header_footer::hf_templates::{HfGallery, gallery_kind};
use uo_docx::session::ops::header_footer::{HeaderFooterCmd, HfAt, HfEnter, HfKind, HfReenter, HfSlot, PageNumberFormatPatch};
use uo_docx::session::ops::page::watermark::WatermarkArg;
use uo_docx::session::ops::page::{GradientStyle, PageCmd, PageFillInput, PageLang};
use uo_docx::session::ops::sections::section_ops::{ColumnPreset, ColumnSpec, Orientation};
use uo_docx::session::ops::sections::{
    ColumnsArg, MarginPreset, PageSetup, PageSize, SectionApplyTo, SectionCmd, SectionType, VerticalAlignment,
};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::ops::text_layout::{HyphenationPatch, LineNumberMode, LineNumberingOptions, LineRestart, TextLayoutCmd};
use uo_docx::session::{ChangeSet, Command, DocPosition, EditContext, Event, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;
use uo_docx::write::emit::background::{WatermarkBox, WatermarkLayout, WatermarkPictureSpec, WatermarkTextSpec};
use uo_docx::write::sig::SigMode;
use uo_testkit::{Capture, Digest, Replay, Stream, repo_root};

fn paras_digest(s: &Session) -> String {
    let doc = s.document();
    let mut paras = doc.body.paras.clone();
    let mut ids = doc.ids.clone();
    uo_docx::session::normalize::normalize(&mut paras, &mut ids);
    let mut sink = Sink(Stream::new());
    for (i, p) in paras.iter().enumerate() {
        sink.0.tag(b'E').u32(u32::try_from(i).unwrap());
        let mut w = CanonWriter::new(&mut sink).with_media(&doc.media);
        p.canon(&mut w);
    }
    sink.0.finish().short()
}

fn pos_json(p: DocPosition) -> String {
    format!("{{\"para\":{},\"offset\":{}}}", p.para, p.offset)
}

fn sel_json(s: &Session) -> String {
    // the TS `s` of the undo state is the body's (the saved one while a context is edited)
    let sel = uo_docx::session::ops::context::saved_selection(s.context()).unwrap_or_else(|| s.selection());
    format!("{{\"anchor\":{},\"focus\":{}}}", pos_json(sel.anchor), pos_json(sel.focus))
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
            other => format!("{other:?}"),
        })
        .collect();
    if e.is_empty() { "-".to_owned() } else { e.join(" ") }
}

fn flags(s: &Session) -> String {
    format!("{}{}{}:{}", u8::from(s.is_dirty()), u8::from(s.can_undo()), u8::from(s.can_redo()), s.edits_since_load())
}

fn hx(u: &[u16]) -> String {
    u.iter().map(|c| format!("{c:x}")).collect::<Vec<_>>().join(".")
}

fn sha(t: &str) -> String {
    Digest::of(t.as_bytes()).short()
}

fn js(x: f64) -> String {
    uo_jsnum::format::to_js_string(x)
}

/// The TS `f(v)`: `-` for `undefined`, `N` for `null`, else `String(v)`.
fn fnum(v: Option<JsNumber>) -> String {
    match v {
        None => "-".to_owned(),
        Some(JsNumber::Null) => "N".to_owned(),
        Some(JsNumber::Num(x)) => js(x),
    }
}

fn fnum_req(v: JsNumber) -> String {
    fnum(Some(v))
}

fn fstr(v: Option<&str>) -> String {
    v.map_or("-".to_owned(), str::to_owned)
}

fn fbool(v: Option<bool>) -> String {
    v.map_or("-".to_owned(), |b| b.to_string())
}

fn ln_s(l: Option<&uo_docx::model::section::LineNumbering>) -> String {
    match l {
        None => "-".to_owned(),
        Some(l) => [fnum(l.count_by), fnum(l.start), fnum(l.distance), fstr(l.restart.as_deref())].join("/"),
    }
}

fn cols_s(c: Option<&uo_docx::model::section::Cols>) -> String {
    match c {
        None => "-".to_owned(),
        Some(c) => format!(
            "{}/{}/{}/{}",
            fnum_req(c.count),
            fnum_req(c.space_twips),
            c.sep,
            c.widths
                .iter()
                .flatten()
                .map(|w| format!("{}.{}", fnum_req(w.width_twips), fnum_req(w.space_twips)))
                .collect::<Vec<_>>()
                .join(";")
        ),
    }
}

fn sp_s(sp: &uo_docx::model::section::SectPr) -> String {
    let head = [
        fnum_req(sp.page_width_twips),
        fnum_req(sp.page_height_twips),
        fnum_req(sp.margin_left_twips),
        fnum_req(sp.margin_right_twips),
        fnum_req(sp.margin_top_twips),
        fnum_req(sp.margin_bottom_twips),
        fstr(sp.r#type.as_deref()),
        fbool(sp.title_pg),
        fnum(sp.header_distance_twips),
        fnum(sp.footer_distance_twips),
        fnum(sp.pg_num_start),
        fstr(sp.pg_num_fmt.as_deref()),
        fnum(sp.pg_num_chap_style),
        fstr(sp.pg_num_chap_sep.as_deref()),
        fstr(sp.v_align.as_deref()),
    ]
    .join("|");
    let borders = match &sp.pg_borders {
        None => "u",
        Some(uo_docx::js::Nullable::Null) => "n",
        Some(_) => "b",
    };
    format!("{head}|{}|{}|{borders}", cols_s(sp.cols.as_ref()), ln_s(sp.ln_num_type.as_ref()))
}

fn refs_s(rs: &[uo_docx::model::section::HfRef]) -> String {
    rs.iter().map(|r| format!("{}.{}.{}", r.r#type, r.rid, r.path)).collect::<Vec<_>>().join(";")
}

fn sec_text(s: &Session) -> String {
    s.document()
        .sections
        .iter()
        .map(|sec| {
            let e = sec.sect.as_deref();
            let (eq, ty, va) = match e {
                Some(e) => (fbool(e.equal_width), fstr(Some(&e.r#type)), fstr(e.v_align.as_deref())),
                None => ("-".to_owned(), "-".to_owned(), "-".to_owned()),
            };
            let ln = e
                .and_then(|e| e.ln_num_type.as_ref())
                .map_or("-".to_owned(), |l| [fnum(l.count_by), fnum(l.start), fnum(l.distance), fstr(l.restart.as_deref())].join("/"));
            let defs = e.and_then(|e| e.col_defs.as_ref()).map_or(String::new(), |d| {
                d.iter().map(|w| format!("{}.{}", fnum_req(w.w), fnum_req(w.space))).collect::<Vec<_>>().join(";")
            });
            format!("{}#{ty}|{va}|{eq}|{ln}|{defs}#{}#{}", sp_s(&sec.sect_pr), refs_s(&sec.header_refs), refs_s(&sec.footer_refs))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn body_text(s: &Session) -> String {
    let st = s.section_state();
    let refs = match &st.body_refs {
        Some(r) if !(r.header_refs.is_empty() && r.footer_refs.is_empty()) => format!(
            "{}/{}",
            r.header_refs.iter().map(|x| format!("{}.{}", x.r#type, x.rid)).collect::<Vec<_>>().join(";"),
            r.footer_refs.iter().map(|x| format!("{}.{}", x.r#type, x.rid)).collect::<Vec<_>>().join(";")
        ),
        _ => "-".to_owned(),
    };
    format!("{}#{refs}", sp_s(&st.body_sect_pr))
}

fn bg_text(s: &Session) -> String {
    let doc = s.document();
    let Some(bg) = doc.background.as_deref() else { return "-".to_owned() };
    let base =
        [fstr(bg.color.as_deref()), fstr(bg.theme_color.as_deref()), fstr(bg.theme_tint.as_deref()), fstr(bg.theme_shade.as_deref())]
            .join("|");
    let Some(fl) = bg.fill.as_deref() else { return base };
    let fp = fl.focus_position.map_or("-".to_owned(), |(a, b)| format!("[{},{}]", fnum_req(a), fnum_req(b)));
    let head = [
        fl.r#type.to_string(),
        fstr(fl.color.as_deref()),
        fstr(fl.color2.as_deref()),
        fnum(fl.angle),
        fnum(fl.focus),
        fp,
        fbool(fl.gradient_center),
        fstr(fl.method.as_deref()),
        fstr(fl.title.as_deref()),
    ]
    .join("/");
    let colors = fl.colors.iter().flatten().map(|c| format!("{}.{}", fnum_req(c.pos), c.color)).collect::<Vec<_>>().join(";");
    let img = fl.image.as_ref().map_or("-".to_owned(), |i| format!("{}.{}", doc.media.get(i.data.0).map_or(0, <[u8]>::len), i.ext));
    format!("{base}|{head}/{colors}/{img}")
}

fn st_text(s: &Session) -> String {
    let doc = s.document();
    let hy = s.hyphenation_settings();
    let dt = s.settings().default_tab.unwrap_or_else(|| doc.formatting.settings.default_tab_stop.to_number());
    format!(
        "{}|{}|{}{}.{}.{}|{}|{}",
        u8::from(s.even_odd_headers()),
        js(dt),
        u8::from(hy.auto),
        u8::from(hy.caps),
        fnum(hy.zone.map(JsNumber::Num)),
        fnum(Some(JsNumber::Num(hy.limit))),
        bg_text(s),
        u8::from(doc.formatting.settings.display_background_shape)
    )
}

fn hfp_text(s: &Session) -> String {
    let doc = s.document();
    s.section_state()
        .hf_parts
        .iter()
        .map(|(k, id)| {
            let (path, texts) = match doc.hf.get(id.index()) {
                Some(h) => (
                    h.path.to_string(),
                    h.story
                        .paras
                        .iter()
                        .map(|p| {
                            let t: Vec<u16> = p
                                .runs
                                .iter()
                                .flat_map(|r| {
                                    if r.ext().is_some_and(|e| e.field.is_some()) { vec![u16::from(b'#')] } else { r.text.units().to_vec() }
                                })
                                .collect();
                            hx(&t)
                        })
                        .collect::<Vec<_>>()
                        .join(";"),
                ),
                None => (String::new(), String::new()),
            };
            format!("{k}|{path}|{texts}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn ctx_text(s: &Session) -> String {
    let EditContext::HeaderFooter { part, .. } = s.context() else { return "-".to_owned() };
    let st = s.section_state();
    let Some((key, _)) = st.hf_parts.iter().find(|(_, id)| *id == part) else { return "-".to_owned() };
    let (kind, rid) = key.split_once(':').unwrap_or((key, ""));
    let path = s.document().hf.get(part.index()).map_or(String::new(), |h| h.path.to_string());
    let ty = s
        .document()
        .sections
        .iter()
        .flat_map(|x| x.header_refs.iter().chain(x.footer_refs.iter()))
        .find(|r| &*r.rid == rid)
        .map_or(String::new(), |r| r.r#type.to_string());
    format!("{kind}:{rid}:{path}:{ty}")
}

/// `wm` of the capture: `watermark()` after the op (the context of the page, `hca`).
fn wm_text(s: &Session, at: HfAt) -> String {
    let Some((info, image)) = s.watermark(at) else { return "-".to_owned() };
    let img = image.map_or("-".to_owned(), |i| i.len().to_string());
    match info {
        WatermarkInfo::Text(t) => [
            "text".to_owned(),
            hx(&t.text.encode_utf16().collect::<Vec<_>>()),
            t.font.to_string(),
            fnum(t.size_pt),
            t.color.to_string(),
            t.semitransparent.to_string(),
            t.layout.to_string(),
            String::new(),
            String::new(),
            "-".to_owned(),
            fnum_req(t.width_pt),
            fnum_req(t.height_pt),
            img,
        ]
        .join("|"),
        WatermarkInfo::Picture(p) => [
            "picture".to_owned(),
            String::new(),
            String::new(),
            "-".to_owned(),
            String::new(),
            "-".to_owned(),
            String::new(),
            p.rid.to_string(),
            hx(&p.title.encode_utf16().collect::<Vec<_>>()),
            p.washout.to_string(),
            fnum_req(p.width_pt),
            fnum_req(p.height_pt),
            img,
        ]
        .join("|"),
    }
}

fn cov_text(s: &Session) -> String {
    let idx: Vec<String> = s.cover_para_indexes().iter().map(ToString::to_string).collect();
    format!("{}:{}", u8::from(s.has_cover_page()), idx.join(","))
}

/// `gal` of the capture (at #0): the galleries' labels in the interface language.
fn gal_text(s: &Session, lang: PageLang) -> String {
    let wg: Vec<String> =
        s.watermark_gallery_entries(lang).iter().map(|e| format!("{}:{}:{}", e.item.id, e.label, e.category_label)).collect();
    let cp: Vec<String> = s.cover_pages(lang).iter().map(|(i, n)| format!("{i}:{n}")).collect();
    let mut parts = vec![wg.join(";"), cp.join(";")];
    for g in ["header", "footer", "pageTop", "pageBottom", "pageMargin", "pageCurrent"] {
        let t: Vec<String> =
            s.header_footer_templates(HfGallery::parse(g).unwrap(), lang).iter().map(|(i, n)| format!("{i}:{n}")).collect();
        parts.push(t.join(";"));
    }
    parts.push(s.watermark_texts(lang).join(","));
    parts.join("|")
}

fn part_of_path(s: &Session, path: &str) -> HfPartId {
    s.document().hf.iter().position(|h| &*h.path == path).map_or(HfPartId(u32::MAX), |i| HfPartId(u32::try_from(i).unwrap()))
}

fn saved(s: &mut Session) -> (String, Option<String>, String) {
    match s.save_docx() {
        Ok(bytes) => {
            let pkg = uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
            let t = pkg.part_text("word/document.xml").unwrap().map(|t| t.into_owned()).unwrap_or_default();
            let mut names: Vec<String> = pkg
                .part_names()
                .filter(|n| {
                    let n = *n;
                    let hf = |p: &str| n.strip_prefix(p).and_then(|r| r.strip_suffix(".xml")).is_some_and(|d| !d.contains('/'));
                    let rel = |p: &str| n.strip_prefix(p).and_then(|r| r.strip_suffix(".xml.rels")).is_some_and(|d| !d.contains('/'));
                    hf("word/header")
                        || hf("word/footer")
                        || n == "word/settings.xml"
                        || n == "word/styles.xml"
                        || n == "[Content_Types].xml"
                        || rel("word/_rels/document")
                        || rel("word/_rels/header")
                        || rel("word/_rels/footer")
                })
                .map(str::to_owned)
                .collect();
            names.sort();
            let parts: Vec<String> =
                names.iter().map(|n| format!("{n}\n{}", pkg.part_text(n).unwrap().map(|t| t.into_owned()).unwrap_or_default())).collect();
            let joined = parts.join("\n#\n");
            if std::env::var("UO_F4_DOCXML").is_ok() {
                println!(
                    "RUST docxml
{t}"
                );
            }
            if std::env::var("UO_F4_PARTS").is_ok() {
                println!("RUST parts\n{joined}");
            }
            let mut media: Vec<String> = pkg
                .part_names()
                .filter(|n| n.starts_with("word/media/"))
                .map(|n| format!("{n}:{}", pkg.part(n).unwrap().map_or(0, <[u8]>::len)))
                .collect();
            media.sort();
            let media = if media.is_empty() { "-".to_owned() } else { media.join(",") };
            (Digest::of(t.as_bytes()).short(), Some(sha(&joined)), media)
        }
        Err(_) => ("err".to_owned(), Some("err".to_owned()), "err".to_owned()),
    }
}

fn units(hex: &str) -> Vec<u16> {
    hex.split('.').filter(|h| !h.is_empty()).map(|h| u16::from_str_radix(h, 16).unwrap()).collect()
}

fn st_of(c: &str) -> SectionType {
    match c {
        "n" => SectionType::NextPage,
        "c" => SectionType::Continuous,
        "k" => SectionType::NextColumn,
        "e" => SectionType::EvenPage,
        _ => SectionType::OddPage,
    }
}

fn ap_of(c: &str) -> SectionApplyTo {
    match c {
        "s" => SectionApplyTo::Section,
        "d" => SectionApplyTo::Document,
        "f" => SectionApplyTo::Forward,
        _ => SectionApplyTo::Selection,
    }
}

fn va_of(c: &str) -> VerticalAlignment {
    match c {
        "t" => VerticalAlignment::Top,
        "c" => VerticalAlignment::Center,
        "b" => VerticalAlignment::Bottom,
        _ => VerticalAlignment::Both,
    }
}

fn gs_of(i: usize) -> GradientStyle {
    [
        GradientStyle::Horizontal,
        GradientStyle::Vertical,
        GradientStyle::DiagonalUp,
        GradientStyle::DiagonalDown,
        GradientStyle::FromCorner,
        GradientStyle::FromCenter,
    ][i]
}

fn ln_opts(spec: &str) -> LineNumberingOptions {
    let p: Vec<&str> = spec.split('.').collect();
    LineNumberingOptions {
        on: p[0] == "1",
        start_at: pf(p[1]),
        count_by: pf(p[2]),
        distance_twips: pf(p[3]),
        restart: match p[4] {
            "p" => LineRestart::Page,
            "s" => LineRestart::Section,
            _ => LineRestart::Continuous,
        },
    }
}

fn pf(x: &str) -> f64 {
    uo_jsnum::parse::to_number(x)
}

fn opt_num(x: &str) -> Option<f64> {
    (x != "-").then(|| pf(x))
}

fn at_of(v: Option<&str>) -> HfAt {
    let Some((sec, ty)) = v.and_then(|v| v.split_once(':')) else { return HfAt::default() };
    HfAt {
        section: sec.parse().unwrap_or(0),
        slot: match ty {
            "first" => HfSlot::First,
            "even" => HfSlot::Even,
            _ => HfSlot::Default,
        },
    }
}

fn kind_of(c: &str) -> Option<HfKind> {
    match c {
        "h" => Some(HfKind::Header),
        "f" => Some(HfKind::Footer),
        _ => None,
    }
}

/// What the capture holds about a step's layout answers.
struct Lay<'a> {
    lang: PageLang,
    wb: Option<&'a str>,
    pi: Option<&'a str>,
    pn: Option<&'a str>,
    hpg: Option<u32>,
    rek: Option<&'a str>,
    hc: Option<&'a str>,
    hp: Option<&'a str>,
    hs: Option<u32>,
    reenter: Option<HfPartId>,
    cr: Option<&'a str>,
    path: Option<&'a str>,
}

/// The element's construction draws one `Math.random()` (the class field `refSeed`), so the TS
/// sequence of the harness starts at the SECOND value of the testkit xorshift: the replay's
/// environment skips the first.
fn env_after_element_creation() -> FixedEnv {
    use uo_docx::session::Env;
    let mut env = FixedEnv::default();
    let _ = env.random_u32();
    env
}

fn lang_of(c: &str) -> PageLang {
    match c {
        "pt" => PageLang::Pt,
        "es" => PageLang::Es,
        _ => PageLang::En,
    }
}

fn unhex(h: &str) -> Vec<u8> {
    (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect()
}

/// `kind:section:variant` of the capture's `rek`/`cr` as the part to create.
fn create_of(cr: &str) -> HfReenter {
    let g: Vec<&str> = cr.split(':').collect();
    HfReenter::Create {
        kind: kind_of(&g[0][..1]).unwrap(),
        section: g[1].parse().unwrap(),
        slot: match g[2] {
            "first" => HfSlot::First,
            "even" => HfSlot::Even,
            _ => HfSlot::Default,
        },
    }
}

/// The part the layout shows after an op of the remainder: the path of the part in edit, which
/// resolves now, else the part the references of section `rek` hold (it is created by the op).
fn part_after(lay: &Lay<'_>) -> HfReenter {
    match lay.reenter {
        Some(p) if p != HfPartId(u32::MAX) => HfReenter::Part(p),
        _ => match lay.rek.filter(|r| *r != "-") {
            Some(r) => create_of(r),
            None => HfReenter::Part(HfPartId(u32::MAX)),
        },
    }
}

fn step(s: &mut Session, code: &str, lay: &Lay<'_>, cx: &mut Cx) -> Result<ChangeSet, String> {
    let f: Vec<&str> = code.split(':').collect();
    let nn = |i: usize| f[i].parse::<u32>().unwrap();
    let hf_before = matches!(s.context(), EditContext::HeaderFooter { .. });
    // the part the page shows after the op: the part in edit afterwards when it exists now, else the
    // part the references of its section hold (the op creates it): unlinks and re-entries alike
    let reenter = if hf_before { Some(part_after(lay)) } else { None };
    if std::env::var("UO_F4_DBG").is_ok() {
        eprintln!(
            "DBG {code}: hf_before {hf_before} reenter {reenter:?} lay.reenter {:?} rek {:?} parts {:?}",
            lay.reenter,
            lay.rek,
            s.document().hf.iter().map(|h| h.path.to_string()).collect::<Vec<_>>()
        );
    }
    let cmd = match f[0] {
        "ty" => Command::Text(TextCmd::Type { text: Utf16::from_units(units(f[1])) }),
        "en" => Command::Text(TextCmd::Enter),
        "bs" => Command::Text(TextCmd::Backspace),
        "se" => Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(nn(1), nn(2)), focus: DocPosition::new(nn(3), nn(4)) }),
        "tk" => Command::Settings(SettingsCmd::SetTracking { on: f[1] == "1" }),
        "ps" => {
            let mut p = PageSetup::default();
            for kv in f[1].split(',') {
                let (k, v) = kv.split_once('=').unwrap();
                match k {
                    "o" => p.orientation = Some(if v == "L" { Orientation::Landscape } else { Orientation::Portrait }),
                    "s" => p.size = Some(if v == "A" { PageSize::A4 } else { PageSize::Letter }),
                    "m" => {
                        p.margins = Some(match v {
                            "n" => MarginPreset::Normal,
                            "r" => MarginPreset::Narrow,
                            _ => MarginPreset::Wide,
                        })
                    }
                    "cm" => {
                        let m: Vec<f64> = v.split('.').map(pf).collect();
                        p.custom_margins = Some([m[0], m[1], m[2], m[3]]);
                    }
                    "st" => p.section_start = Some(st_of(v)),
                    "va" => p.vertical_alignment = Some(va_of(v)),
                    "ln" => p.line_numbering = Some(ln_opts(v)),
                    "ap" => p.apply_to = Some(ap_of(v)),
                    _ => panic!("ps {k}"),
                }
            }
            Command::Section(SectionCmd::SetPageSetup(Box::new(p)))
        }
        "sb" => Command::Section(SectionCmd::InsertSectionBreak { kind: st_of(f[1]) }),
        "cb" => Command::Section(SectionCmd::InsertColumnBreak),
        "pgb" => Command::Section(SectionCmd::InsertPageBreak),
        "bp" => Command::Section(SectionCmd::InsertBlankPage),
        "col" => Command::Section(SectionCmd::SetColumns(Box::new(ColumnsArg::Preset(match f[1] {
            "1" => ColumnPreset::One,
            "2" => ColumnPreset::Two,
            "3" => ColumnPreset::Three,
            "l" => ColumnPreset::Left,
            _ => ColumnPreset::Right,
        })))),
        "cs" => Command::Section(SectionCmd::SetColumns(Box::new(ColumnsArg::Spec {
            spec: ColumnSpec {
                count: pf(f[1]),
                space_twips: opt_num(f[2]),
                widths: (f[6] == "1").then(|| vec![(3000.0, 300.0), (2500.0, 200.0), (2000.0, 100.0), (900.0, 0.0)]),
                sep: f[3] == "1",
            },
            apply_to: Some(ap_of(f[4])),
            start_new_column: f[5] == "1",
        }))),
        "ss" => Command::Section(SectionCmd::SetSectionStart { kind: st_of(f[1]), apply_to: ap_of(f[2]), hf_section: lay.hs }),
        "va" => Command::Section(SectionCmd::SetVerticalAlignment { v: va_of(f[1]), apply_to: ap_of(f[2]), hf_section: lay.hs }),
        "ln" => Command::Section(SectionCmd::SetLineNumbering {
            mode: match f[1] {
                "n" => LineNumberMode::None,
                "c" => LineNumberMode::Continuous,
                "p" => LineNumberMode::Page,
                _ => LineNumberMode::Section,
            },
            hf_section: lay.hs,
        }),
        "lo" => Command::Section(SectionCmd::SetLineNumberingOptions { o: ln_opts(f[1]), apply_to: ap_of(f[2]), hf_section: lay.hs }),
        "ls" => Command::Section(SectionCmd::ToggleLineNumberSuppression),
        "pb" => Command::Section(SectionCmd::SetPageBorders { borders: None, document_wide: f[1] == "d" }),
        "dfp" => Command::HeaderFooter(HeaderFooterCmd::SetDifferentFirstPage { on: f[1] == "1", at: at_of(lay.hc), reenter }),
        "doe" => Command::HeaderFooter(HeaderFooterCmd::SetDifferentOddEven { on: f[1] == "1", reenter }),
        "hfd" => Command::HeaderFooter(HeaderFooterCmd::SetHeaderFooterDistance {
            header: opt_num(f[1]),
            footer: opt_num(f[2]),
            at: at_of(lay.hc),
            reenter,
        }),
        "ltp" => {
            Command::HeaderFooter(HeaderFooterCmd::SetLinkToPrevious { on: f[1] == "1", kind: kind_of(f[2]), at: at_of(lay.hc), reenter })
        }
        "rh" => Command::HeaderFooter(HeaderFooterCmd::RemoveHeaderFooter { kind: kind_of(f[1]).unwrap(), at: at_of(lay.hp) }),
        "rpn" => Command::HeaderFooter(HeaderFooterCmd::RemovePageNumbers { at: at_of(lay.hp) }),
        "pnf" => Command::HeaderFooter(HeaderFooterCmd::SetPageNumberFormat {
            f: PageNumberFormatPatch {
                format: (f[1] != "-").then(|| f[1].to_owned()),
                chapter: match f[2] {
                    "-" => None,
                    "none" => Some(None),
                    l => Some(Some((pf(l), "hyphen".to_owned()))),
                },
                start: match f[3] {
                    "-" => None,
                    "none" => Some(None),
                    l => Some(Some(pf(l))),
                },
            },
            at: at_of(lay.hc),
            reenter,
        }),
        "dt" => Command::TextLayout(TextLayoutCmd::SetDefaultTabStop { twips: pf(f[1]) }),
        "hy" => Command::TextLayout(TextLayoutCmd::SetHyphenation { auto: f[1] == "1" }),
        "ho" => {
            let p: Vec<&str> = f[1].split('.').collect();
            let flag = |x: &str| (x != "-").then(|| x == "1");
            Command::TextLayout(TextLayoutCmd::SetHyphenationOptions {
                o: HyphenationPatch { auto: flag(p[0]), caps: flag(p[1]), zone_twips: opt_num(p[2]), limit: opt_num(p[3]) },
            })
        }
        "pc" => Command::Page(PageCmd::SetPageColor {
            color: match f[1] {
                "none" => None,
                t if t.starts_with("t_") => {
                    let g: Vec<&str> = t.split('_').collect();
                    Some(ColorArg::Theme(ThemeColorRef { theme_color: g[1].to_owned(), tint: opt_num(g[2]), shade: opt_num(g[3]) }))
                }
                h => Some(ColorArg::Hex(h.to_owned())),
            },
        }),
        "pf" => {
            let g: Vec<&str> = f[1].split('_').collect();
            let fill = match g[0] {
                "none" => None,
                "g1" => Some(PageFillInput::GradientOne {
                    color: g[1].into(),
                    degree: pf(g[2]),
                    style: gs_of(g[3].parse().unwrap()),
                    variant: pf(g[4]),
                }),
                "g2" => Some(PageFillInput::GradientTwo {
                    color: g[1].into(),
                    color2: g[2].into(),
                    style: gs_of(g[3].parse().unwrap()),
                    variant: pf(g[4]),
                }),
                "gp" => Some(PageFillInput::GradientPreset { preset: g[1].into(), style: gs_of(g[2].parse().unwrap()), variant: pf(g[3]) }),
                "pat" => Some(PageFillInput::Pattern { pattern: g[1].into(), color: g[2].into(), background: g[3].into() }),
                _ => Some(PageFillInput::Picture {
                    texture: g[1] == "1",
                    data: std::sync::Arc::from(vec![1u8, 2, 3].into_boxed_slice()),
                    ext: "PNG".into(),
                    name: "a.b.png".into(),
                }),
            };
            Command::Page(PageCmd::SetPageFill { fill, lang: lay.lang })
        }
        "wm" => {
            let arg = match f[1] {
                "g" => WatermarkArg::Gallery(f[2].to_owned()),
                "t" => {
                    let spec = WatermarkTextSpec {
                        text: String::from_utf16_lossy(&units(f[2])),
                        font: f[3].to_owned(),
                        size_pt: opt_num(f[4]),
                        color: f[5].to_owned(),
                        semitransparent: f[6] == "1",
                        layout: if f[7] == "d" { WatermarkLayout::Diagonal } else { WatermarkLayout::Horizontal },
                        lang: None,
                    };
                    let measured_box = lay.wb.filter(|w| *w != "-").map(|w| {
                        let (a, b) = w.split_once('|').unwrap();
                        WatermarkBox { width_pt: pf(a), height_pt: pf(b) }
                    });
                    WatermarkArg::Text { spec, measured_box }
                }
                _ => {
                    let spec = WatermarkPictureSpec {
                        data: std::sync::Arc::from(unhex(f[2]).into_boxed_slice()),
                        ext: f[5].to_owned(),
                        title: f[6].to_owned(),
                        scale: opt_num(f[3]),
                        washout: f[4] == "1",
                    };
                    let pixel_info = lay.pi.filter(|p| *p != "-").map(|p| {
                        let v: Vec<f64> = p.split(',').map(pf).collect();
                        [v[0], v[1], v[2]]
                    });
                    WatermarkArg::Picture { spec, pixel_info }
                }
            };
            Command::Page(PageCmd::SetWatermark { spec: Box::new(arg), at: at_of(lay.hc), lang: lay.lang, reenter })
        }
        "rwm" => Command::Page(PageCmd::RemoveWatermark { reenter }),
        "cv" => Command::Page(PageCmd::InsertCoverPage { id: f[1].to_owned(), lang: lay.lang }),
        "rcv" => Command::Page(PageCmd::RemoveCoverPage),
        "hft" => {
            let gallery = HfGallery::parse(f[1]).unwrap();
            let editing = ctx_text(s);
            let same_kind = gallery_kind(gallery).is_some_and(|k| editing.starts_with(k.as_str()));
            let enter = (gallery_kind(gallery).is_some() && !same_kind)
                .then(|| lay.hpg.map(|page| HfEnter { part: part_after(lay), page }))
                .flatten();
            let page_numbers = lay.pn.filter(|p| *p != "-").map(|p| {
                let (a, b) = p.split_once(':').unwrap();
                (pf(a), pf(b))
            });
            Command::HeaderFooter(HeaderFooterCmd::InsertTemplate { gallery, id: f[2].to_owned(), lang: lay.lang, enter, page_numbers })
        }
        "ehf" => {
            // the TS no-op when already in edit
            if hf_before {
                return Ok(ChangeSet::default());
            }
            let mut last = ChangeSet::default();
            if let Some(cr) = lay.cr.filter(|c| *c != "-") {
                let g: Vec<&str> = cr.split(':').collect();
                let slot = match g[2] {
                    "first" => HfSlot::First,
                    "even" => HfSlot::Even,
                    _ => HfSlot::Default,
                };
                let c = Command::HeaderFooter(HeaderFooterCmd::CreateHeaderFooterPart {
                    kind: kind_of(&g[0][..1]).unwrap(),
                    section: g[1].parse().unwrap(),
                    slot,
                });
                match s.exec(c, &mut cx.cx()) {
                    Ok(Outcome::Done(cs)) => last = cs,
                    other => return Err(format!("{code}: create {other:?}")),
                }
            }
            if let Some(path) = lay.path.filter(|p| *p != "-") {
                let part = part_of_path(s, path);
                match s.exec(Command::Context(ContextCmd::EnterHeaderFooter { part, page: 0 }), &mut cx.cx()) {
                    Ok(Outcome::Done(cs)) => {
                        let mut merged = last;
                        merged.events.extend(cs.events);
                        return Ok(merged);
                    }
                    other => return Err(format!("{code}: enter {other:?}")),
                }
            }
            return Ok(last);
        }
        "xhf" => Command::Context(ContextCmd::ExitHeaderFooter),
        "un" => return Ok(s.undo_reentering(&mut cx.cx(), reenter)),
        "re" => return Ok(s.redo_reentering(&mut cx.cx(), reenter)),
        _ => panic!("step {code}"),
    };
    match s.exec(cmd, &mut cx.cx()) {
        Ok(Outcome::Done(cs)) => Ok(cs),
        Ok(Outcome::NeedsJobs(_)) => panic!("no jobs in F4 scripts"),
        Err(e) => Err(format!("{code}: {e}")),
    }
}

/// A step that reaches a rule of a D2b family that has not landed (the hooks of `session::ops::text`).
fn d2b_hook(s: &Session, code: &str) -> Option<&'static str> {
    let sel = s.selection();
    if sel.anchor != sel.focus {
        return None;
    }
    let p = s.paras()?.get(sel.focus.para as usize)?;
    let len = uo_docx::session::text::para_length(p);
    let style = p.props.style_id.as_deref().and_then(|id| s.document().styles.get(id));
    let sheet = &s.document().formatting.styles.styles;
    let mut id = p.props.style_id.clone();
    let mut style_num = false;
    for _ in 0..32 {
        let Some(def) = id.as_deref().and_then(|i| sheet.get(i)) else { break };
        if def.p_pr.as_deref().is_some_and(|pp| pp.num_pr.is_some()) {
            style_num = true;
            break;
        }
        id = def.based_on.clone();
    }
    let numbered = p.props.numbering.is_some() || style_num;
    let list_like = numbered
        || style.and_then(|st| st.name.as_deref()).is_some_and(|n| n.eq_ignore_ascii_case("list paragraph"))
        || p.props.indent_left_twips == Some(JsNumber::Num(360.0));
    match code {
        "en" if numbered && len == 0 => Some("D2b-F2 list rule: listEnterOnEmpty (D2a-T4)"),
        "bs" if list_like && sel.focus.offset == 0 => Some("D2b-F2 list rule: listBackspaceAtStart (D2a-T4)"),
        _ => None,
    }
}

#[test]
fn d2b_f4_session_matches_the_element() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(std::env::var("UO_F4_CAPTURE").map_or_else(|_| dir.join("d2b-f4-session.capture"), std::path::PathBuf::from))
        .unwrap();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let steps: usize = cap.meta("steps").unwrap().parse().unwrap();
    let mut replay = Replay::new(&cap);
    let mut cx = Cx::new();
    let mut docs = 0;
    let only = std::env::var("UO_F4_DOC").ok();
    let mut sink_media = 0usize;
    for rec in cap.records().iter().filter(|r| r.key.ends_with("#0")) {
        let rel = rec.key.trim_end_matches("#0");
        if only.as_deref().is_some_and(|o| o != rel) {
            continue;
        }
        let bytes = std::fs::read(root.join(rel)).unwrap();
        let mut s =
            Session::open(bytes, OpenOptions { sig_mode: SigMode::Ts, ..OpenOptions::default() }, Box::new(env_after_element_creation()))
                .unwrap();
        s.exec(Command::Settings(SettingsCmd::SetAuthor { name: "A".into() }), &mut cx.cx()).unwrap();
        let mut pending: Option<&str> = None;
        let lang = lang_of(cap.get(&format!("{rel}#0"), "lang").unwrap_or("pt"));
        for j in 0..=steps {
            let key = format!("{rel}#{j}");
            let Some(code) = cap.get(&key, "op") else { break };
            replay.check(&key, "op", code);
            if pending.is_none() {
                pending = d2b_hook(&s, code);
            }
            let lay = Lay {
                lang,
                wb: cap.get(&key, "wb"),
                pi: cap.get(&key, "pi"),
                pn: cap.get(&key, "pn"),
                hpg: cap.get(&key, "hpg").and_then(|h| h.parse().ok()),
                rek: cap.get(&key, "rek"),
                hc: cap.get(&key, "hc"),
                hp: cap.get(&key, "hp"),
                hs: cap.get(&key, "hs").and_then(|h| h.parse().ok()),
                reenter: cap.get(&key, "reenter").filter(|p| *p != "-").map(|p| part_of_path(&s, p)),
                cr: cap.get(&key, "cr"),
                path: cap.get(&key, "reenter"),
            };
            for f in [
                "hc",
                "hp",
                "hs",
                "reenter",
                "cr",
                "wb",
                "pi",
                "pn",
                "hpg",
                "rek",
                "pjson",
                "savectx",
                "docxml.text",
                "sec.text",
                "body.text",
                "hfp.text",
                "parts.text",
                "paras.text",
            ] {
                if cap.get(&key, f).is_some() {
                    replay.skip(&key, f);
                }
            }
            let mut ev = None;
            if j == 0 {
                replay.skip(&key, "ev");
            } else {
                let cs = match step(&mut s, code, &lay, &mut cx) {
                    Ok(cs) => cs,
                    Err(e) => panic!("{rel}#{j}: {e}"),
                };
                ev = Some(ev_text(&cs));
                if std::env::var("UO_F4_DBG").is_ok() {
                    eprintln!(
                        "DBG after {code}: body {} | hfp {} | ctx {}",
                        body_text(&s),
                        hfp_text(&s).replace(char::from(10), ";"),
                        ctx_text(&s)
                    );
                }
            }
            let mut facets = vec![
                ("paras", paras_digest(&s)),
                ("sel", sel_json(&s)),
                ("anc", anc_json(&s)),
                ("flags", flags(&s)),
                ("sec", sha(&sec_text(&s))),
                ("body", sha(&body_text(&s))),
                ("st", st_text(&s)),
                ("hfp", sha(&hfp_text(&s))),
                ("ctx", ctx_text(&s)),
            ];
            if let Some(ev) = ev {
                facets.push(("ev", ev));
            }
            facets.push(("wm", wm_text(&s, at_of(cap.get(&key, "hca")))));
            facets.push(("cov", cov_text(&s)));
            replay.skip(&key, "hca");
            if j == 0 {
                facets.push(("gal", sha(&gal_text(&s, lang))));
                replay.skip(&key, "lang");
            }
            if cap.get(&key, "docxml").is_some() {
                let (d, parts, media) = saved(&mut s);
                // `saveDocx` in header mode returns to the page, in the part the layout shows there now (the
                // layout's answer, D2a-C2): the replay enters the part the TS recorded
                if let (Some(sc), EditContext::HeaderFooter { part, page, .. }) =
                    (cap.get(&key, "savectx").filter(|p| *p != "-"), s.context().clone())
                {
                    let (path, rek) = sc.split_once('|').unwrap();
                    let mut want = part_of_path(&s, path);
                    if want == HfPartId(u32::MAX)
                        && let HfReenter::Create { kind, section, slot } = create_of(rek)
                    {
                        // the part is new: the save's return to the page makes it, as `enterHeaderFooter` does
                        let _ =
                            s.exec(Command::HeaderFooter(HeaderFooterCmd::CreateHeaderFooterPart { kind, section, slot }), &mut cx.cx());
                        want = part_of_path(&s, path);
                    }
                    if want != part && want != HfPartId(u32::MAX) {
                        let _ = s.exec(Command::Context(ContextCmd::EnterHeaderFooter { part: want, page }), &mut cx.cx());
                    }
                }
                facets.push(("docxml", d));
                if let Some(p) = parts {
                    facets.push(("parts", p));
                }
                facets.push(("media", media));
                sink_media += 1;
            }
            if std::env::var("UO_F4_DUMP").is_ok_and(|d| d == key) {
                let doc = s.document();
                let mut paras = doc.body.paras.clone();
                let mut ids = doc.ids.clone();
                uo_docx::session::normalize::normalize(&mut paras, &mut ids);
                let each: Vec<String> = paras
                    .iter()
                    .map(|p| {
                        let mut sink = Sink(Stream::new());
                        let mut w = CanonWriter::new(&mut sink).with_media(&doc.media);
                        p.canon(&mut w);
                        sink.0.finish().short()[..6].to_owned()
                    })
                    .collect();
                println!(
                    "RUST paras.text
{}",
                    each.join(",")
                );
                println!("RUST sec\n{}\nRUST body\n{}\nRUST hfp\n{}", sec_text(&s), body_text(&s), hfp_text(&s));
                for (i, p) in paras.iter().enumerate() {
                    println!("RUST pjson {i} {}", uo_docx::js::JsonWriter::stringify(p, None).unwrap_or_default());
                }
                for f in ["sec.text", "body.text", "hfp.text", "parts.text", "paras.text"] {
                    println!("TS {f}\n{}", cap.get(&key, f).unwrap_or("-").replace('\u{23ce}', "\n"));
                }
            }
            for (f, actual) in facets {
                match pending {
                    Some(why) if j > 0 => {
                        replay.accept(&key, f, &actual, why);
                    }
                    _ => {
                        replay.check(&key, f, &actual);
                    }
                }
            }
        }
        docs += 1;
    }
    let report = replay.finish();
    println!("d2b-f4-session: {docs} documents, {sink_media} saves checked, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_D2A_ALL").map_or(40, |_| 100_000)));
    }
    report.assert_clean();
}
