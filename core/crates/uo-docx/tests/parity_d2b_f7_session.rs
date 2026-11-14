//! Parity of D2b-F7 (drawing objects, text boxes, pictures) against the frozen `<universal-write>`
//! element in Chromium (G17, with the uo-jsnum math override): replays
//! `tests/parity/d2b-f7-session.capture`, written by `js/capture-d2b-f7-session.mjs`. Seeded
//! scripts over corpus documents (those with drawings first) mix typing with the selection of
//! objects (by index, inline, through the selection pane), the shape commands (fill, outline,
//! style, geometry, effects, text options, delete), text boxes (the gallery, links, the default
//! box), the picture commands (wrap, position, size, rotation, flips, alt text, arrange, crops,
//! borders, effects, transparency, recolour, reset, layout, align) and the two-phase ones (insert,
//! change, adjust, compress, the contour of a tight wrap) with undo/redo.
//!
//! Facets per step: the D2a ones (`paras`, `sel`, `anc`, `ev`, `flags`, `docxml`) and F7's: `fs`
//! (the selected object's index), `tb` (the box in edit), `pane` (the selection pane), `st` (the
//! model values of `selectedShape()`/`selectedImage()`), `enc` (the pixels handed to an encoder),
//! `n` (what `compressImages` returned), `cnt` (`selectedObjectCount()`).
//!
//! What the TS reads from the layout and from the browser's pixels is in the capture (`fx`): the
//! replay turns it into the facts the commands take (`ObjFacts`, `ObjFrame`, `PlaceAt`) and into
//! the answers of the jobs, in order.
//!
//! Accepted by name: the events of the install (step 0), the list rules of D2b-F2 (as the D2a
//! replay), and the `enc` facet of `compressImages` (F7-D1: the TS resamples with the browser's
//! `drawImage`, the Rust with an area average; the replay feeds the TS's encoded bytes, so the
//! model and the XML are compared).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f7-session.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::support;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use d2a_support::Cx;
use support::Sink;
use uo_docx::canon::CanonWriter;
use uo_docx::js::{JsValue, Nullable};
use uo_docx::model::picture::PicColor;
use uo_docx::session::ops::context::EditContext;
use uo_docx::session::ops::drawing::{AlignKind, DrawingCmd, ObjFacts, ObjFrame, ObjRect, PaneKind, TextboxOptions};
use uo_docx::session::ops::pictures::{AdjustArg, ArrangeOp, EffectKind, LayoutInput, LayoutPos, PictureCmd, RecolorArg};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{ChangeSet, Command, DocPosition, Env, Event, FixedEnv, Job, JobResult, LocalNow, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;
use uo_docx::write::emit::fx::PicWrapKind;
use uo_docx::write::emit::object::ShapeFill;
use uo_docx::write::sig::SigMode;
use uo_testkit::{Capture, Digest, Replay, Stream, repo_root};

const EFFECTS: [EffectKind; 6] =
    [EffectKind::Shadow, EffectKind::Reflection, EffectKind::Glow, EffectKind::SoftEdge, EffectKind::Bevel, EffectKind::Rotation];
const EFFECT_NAMES: [&str; 6] = ["shadow", "reflection", "glow", "softEdge", "bevel", "rotation"];
const PRESET_IDS: [&[&str]; 6] = [
    &["outer-br", "outer-b", "inner-t", "nope"],
    &["tight-4", "half-4", "full-8", "nope"],
    &["glow-0-1", "glow-3-6", "glow-9-9", "nope", ""],
    &["5", "10", "25", "0", "x"],
    &["circle", "relaxedInset", "hardEdge", "nope", ""],
    &["x"],
];
const RECOLORS: [&str; 6] = ["grayscale", "washout", "blackwhite", "sepia", "nope", "#336699"];
const ARRANGE: [(&str, ArrangeOp); 6] = [
    ("front", ArrangeOp::Front),
    ("back", ArrangeOp::Back),
    ("forward", ArrangeOp::Forward),
    ("backward", ArrangeOp::Backward),
    ("frontOfText", ArrangeOp::FrontOfText),
    ("behindText", ArrangeOp::BehindText),
];
const ALIGNS: [(&str, AlignKind); 8] = [
    ("left", AlignKind::Left),
    ("center", AlignKind::Center),
    ("right", AlignKind::Right),
    ("top", AlignKind::Top),
    ("middle", AlignKind::Middle),
    ("bottom", AlignKind::Bottom),
    ("distributeH", AlignKind::DistributeH),
    ("distributeV", AlignKind::DistributeV),
];
const WRAPS: [(&str, PicWrapKind); 7] = [
    ("inline", PicWrapKind::Inline),
    ("square", PicWrapKind::Square),
    ("tight", PicWrapKind::Tight),
    ("through", PicWrapKind::Through),
    ("topAndBottom", PicWrapKind::TopAndBottom),
    ("behind", PicWrapKind::Behind),
    ("front", PicWrapKind::Front),
];

// ─────────────────────────── helpers of the D2a/F6 replays ───────────────────────────

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

fn sel_pair(a: DocPosition, f: DocPosition) -> String {
    format!("{{\"anchor\":{},\"focus\":{}}}", pos_json(a), pos_json(f))
}

/// `state.s`: the selection of the story outside a text box in edit.
fn sel_json(s: &Session) -> String {
    match s.context() {
        EditContext::Textbox { saved, .. } => sel_pair(saved.anchor, saved.focus),
        _ => {
            let sel = s.selection();
            sel_pair(sel.anchor, sel.focus)
        }
    }
}

/// `state.te`: the box in edit with the selection inside it.
fn tb_text(s: &Session) -> String {
    match s.context() {
        EditContext::Textbox { host, focus, page, .. } => {
            let sel = s.selection();
            format!("{host}|{focus}|{page}|{}", sel_pair(sel.anchor, sel.focus))
        }
        _ => "-".to_owned(),
    }
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
            Event::RevisionsChanged => "uw-revisions-changed:".to_owned(),
            Event::CommentsChanged => "uw-comments-changed:".to_owned(),
            Event::BookmarksChange => "uw-bookmarks-change:".to_owned(),
            Event::Markup => "uw-markup:".to_owned(),
            other => format!("{other:?}"),
        })
        .collect();
    if e.is_empty() { "-".to_owned() } else { e.join(" ") }
}

fn flags(s: &Session) -> String {
    format!("{}{}{}:{}", u8::from(s.is_dirty()), u8::from(s.can_undo()), u8::from(s.can_redo()), s.edits_since_load())
}

fn docxml(s: &mut Session) -> String {
    match s.save_docx() {
        Ok(bytes) => {
            let pkg = uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
            let t = pkg.part_text("word/document.xml").unwrap().map(|t| t.into_owned()).unwrap_or_default();
            Digest::of(t.as_bytes()).short()
        }
        Err(_) => "err".to_owned(),
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

fn hstr(hex: &str) -> String {
    String::from_utf16_lossy(&units(hex))
}

fn num(s: &str) -> f64 {
    uo_jsnum::parse::to_number(s)
}

fn js(x: f64) -> String {
    uo_jsnum::format::to_js_string(x)
}

fn sha(t: &str) -> String {
    Digest::of(t.as_bytes()).short()
}

fn b64(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).unwrap());
        }
    }
    out
}

fn fnv(px: &[u8]) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for b in px {
        h ^= u32::from(*b);
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{h:x}")
}

/// The environment of the session with a shared counter of the random values it gave: the page's
/// `Math.random` is also consumed by the editor itself, so the replay advances the generator to the
/// count the capture says before each step and compares the count after it (`rc`).
#[derive(Clone)]
struct SharedEnv(Rc<RefCell<(FixedEnv, u64)>>);

impl SharedEnv {
    fn new() -> SharedEnv {
        SharedEnv(Rc::new(RefCell::new((FixedEnv::default(), 0))))
    }

    fn calls(&self) -> u64 {
        self.0.borrow().1
    }

    /// Consumes values until `n` have been given.
    fn advance_to(&self, n: u64) {
        let mut g = self.0.borrow_mut();
        while g.1 < n {
            g.0.random_u32();
            g.1 += 1;
        }
    }
}

impl Env for SharedEnv {
    fn now(&self) -> LocalNow {
        self.0.borrow().0.now()
    }

    fn random_u32(&mut self) -> u32 {
        let mut g = self.0.borrow_mut();
        g.1 += 1;
        g.0.random_u32()
    }
}

// ─────────────────────────── the fact log of a step ───────────────────────────

#[derive(Default)]
struct Fx {
    /// `selectedFloat()` results, in call order (`None` = null): x y w h page inline
    sf: Vec<Option<[f64; 6]>>,
    /// `selectedFloats()` results
    sfs: Vec<Vec<[f64; 6]>>,
    /// `engObjectFrame()` results
    fr: Vec<Option<ObjFrame>>,
    /// `engObjectOf()` results: the page
    eo: Vec<Option<u32>>,
    bmp: VecDeque<(u32, u32)>,
    gid: VecDeque<(u32, u32, Vec<u8>)>,
    blob: VecDeque<Vec<u8>>,
}

impl Fx {
    fn parse(t: &str) -> Fx {
        let mut fx = Fx::default();
        if t == "-" {
            return fx;
        }
        for e in t.split('|') {
            let f: Vec<&str> = e.split(';').collect();
            let nums = |from: usize| -> Vec<f64> { f[from..].iter().map(|x| num(x)).collect() };
            match f[0] {
                "sf" => fx.sf.push((f[1] != "null").then(|| <[f64; 6]>::try_from(nums(1)).unwrap())),
                "sfs" => fx.sfs.push(nums(1).chunks(6).map(|c| <[f64; 6]>::try_from(c).unwrap()).collect()),
                "fr" => fx.fr.push((f[1] != "null").then(|| {
                    let n = nums(1);
                    ObjFrame {
                        page_w: n[0],
                        page_h: n[1],
                        mar_l: n[2],
                        mar_r: n[3],
                        mar_t: n[4],
                        mar_b: n[5],
                        col_x: n[6],
                        col_w: n[7],
                        body_top: n[8],
                        para_top: n[9],
                    }
                })),
                "eo" => fx.eo.push((f[1] != "null").then(|| u32::try_from(num(f[5]) as i64).unwrap_or(0))),
                "bmp" => fx.bmp.push_back((num(f[1]) as u32, num(f[2]) as u32)),
                "gid" => fx.gid.push_back((num(f[1]) as u32, num(f[2]) as u32, b64(f[3]))),
                "blob" => fx.blob.push_back(b64(f[3])),
                _ => {}
            }
        }
        fx
    }

    fn rect(a: [f64; 6]) -> ObjRect {
        ObjRect { x: a[0], y: a[1], w: a[2], h: a[3] }
    }

    /// The facts of the first non-null `selectedFloat` and the first non-null `engObjectFrame`.
    fn facts(&self) -> ObjFacts {
        ObjFacts { rect: self.sf.iter().flatten().next().map(|a| Fx::rect(*a)), frame: self.fr.iter().flatten().next().copied() }
    }

    /// The selected floating objects of the FIRST `selectedFloats()` call of the step (the one the
    /// align command reads; a later call is the inner command looking again), with their frames.
    fn rects(&self) -> Vec<ObjFacts> {
        let list: Vec<[f64; 6]> = self.sfs.first().into_iter().flatten().copied().filter(|a| a[5] == 0.0).collect();
        let frames: Vec<ObjFrame> = self.fr.iter().flatten().copied().collect();
        if list.is_empty() {
            return self.sf.iter().flatten().map(|a| ObjFacts { rect: Some(Fx::rect(*a)), frame: frames.first().copied() }).collect();
        }
        list.iter().enumerate().map(|(i, a)| ObjFacts { rect: Some(Fx::rect(*a)), frame: frames.get(i).copied() }).collect()
    }
}

fn color(i: usize) -> PicColor {
    let c = |kind: &str, val: &str| PicColor { kind: kind.into(), val: val.into(), ..PicColor::default() };
    [c("srgbClr", "FF0000"), c("schemeClr", "accent1"), c("srgbClr", "336699"), c("prstClr", "black")][i % 4].clone()
}

fn eff_preset(kind: usize, i: &str) -> Option<String> {
    let l = PRESET_IDS[kind];
    let i = i.parse::<usize>().unwrap();
    l.get(i).map(|x| (*x).to_owned())
}

fn images(cap: &Capture) -> Vec<(String, Vec<u8>)> {
    (0..16)
        .map_while(|k| {
            let key = format!("img#{k}");
            cap.get(&key, "ext").map(|e| (e.to_owned(), b64(cap.get(&key, "b64").unwrap())))
        })
        .collect()
}

/// The decode and encode jobs of a command answered from the fact log.
struct Run<'a> {
    s: &'a mut Session,
    cx: &'a mut Cx,
    fx: Fx,
    encs: Vec<String>,
}

impl Run<'_> {
    fn finish(&mut self, mut out: Outcome) -> Result<ChangeSet, String> {
        for _ in 0..8 {
            match out {
                Outcome::Done(cs) => return Ok(cs),
                Outcome::NeedsJobs(jobs) => {
                    let mut last = None;
                    for j in &jobs {
                        let r = match j {
                            Job::DecodeImage { .. } => match (self.fx.gid.pop_front(), self.fx.bmp.front().copied()) {
                                (Some((w, h, px)), _) => {
                                    if std::env::var("UO_F7_JOBS").is_ok() {
                                        println!("JOB decode answered {w}x{h}");
                                    }
                                    JobResult::Image { width: w, height: h, rgba: Some(Arc::from(px)) }
                                }
                                (None, Some((w, h))) => {
                                    self.fx.bmp.pop_front();
                                    JobResult::Image { width: w, height: h, rgba: None }
                                }
                                _ => JobResult::Failed,
                            },
                            Job::EncodeImage { width, height, rgba, .. } => {
                                self.encs.push(format!("{width}x{height}:{}", fnv(rgba)));
                                self.fx.blob.pop_front().map_or(JobResult::Failed, |b| JobResult::Bytes(Arc::from(b)))
                            }
                            _ => JobResult::Failed,
                        };
                        last = Some(self.s.complete(j.id(), r, &mut self.cx.cx()).map_err(|e| e.to_string())?);
                    }
                    match last {
                        Some(o) => out = o,
                        None => return Err("a command without jobs".into()),
                    }
                }
            }
        }
        Err("too many rounds of jobs".into())
    }

    fn exec(&mut self, c: Command) -> Result<ChangeSet, String> {
        let out = self.s.exec(c, &mut self.cx.cx()).map_err(|e| e.to_string())?;
        self.finish(out)
    }

    fn pic(&mut self, c: PictureCmd) -> Result<ChangeSet, String> {
        self.exec(Command::Picture(c))
    }

    fn dr(&mut self, c: DrawingCmd) -> Result<ChangeSet, String> {
        self.exec(Command::Drawing(c))
    }
}

fn fmt_state(o: Option<uo_docx::session::ops::drawing::ObjectState>, image: bool) -> String {
    let Some(s) = o else { return "-".to_owned() };
    let b = |x: bool| u8::from(x).to_string();
    let kind = if image {
        "image"
    } else {
        match s.kind {
            PaneKind::Textbox => "textbox",
            PaneKind::Group => "group",
            _ => "shape",
        }
    };
    let t = s.text.as_ref().map_or("-".to_owned(), |t| {
        format!(
            "{},{},{},{}|{}|{}|{}|{}|{}",
            js(t.margins_emu[0]),
            js(t.margins_emu[1]),
            js(t.margins_emu[2]),
            js(t.margins_emu[3]),
            t.anchor,
            t.direction,
            b(t.wrap),
            t.autofit,
            b(t.upright)
        )
    });
    let link = if !image && s.kind == PaneKind::Textbox { format!("{}{}", b(s.link_next), b(s.link_previous)) } else { "-".to_owned() };
    [
        kind.to_owned(),
        b(s.inline),
        s.wrap.clone(),
        js(s.cx),
        js(s.cy),
        js(s.rot),
        b(s.flip_h),
        b(s.flip_v),
        b(s.lock_aspect),
        b(s.move_with_text),
        b(s.lock_anchor),
        hs(&s.name),
        hs(&s.alt_text),
        b(s.decorative),
        if image { String::new() } else { hs(&s.geometry) },
        s.style_index.to_string(),
        b(s.editing_text),
        t,
        link,
    ]
    .join("|")
}

fn pane_text(s: &Session) -> String {
    s.selection_pane_items()
        .iter()
        .map(|i| {
            format!(
                "{}|{}|{}|{}|{}",
                hs(&i.key),
                hs(&i.name),
                u8::from(i.hidden),
                u8::from(i.selected),
                match i.kind {
                    PaneKind::Image => "image",
                    PaneKind::Textbox => "textbox",
                    PaneKind::Shape => "shape",
                    PaneKind::Group => "group",
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn fs_text(s: &Session) -> String {
    match s.context() {
        EditContext::Cell { .. } | EditContext::Note { .. } => "-".to_owned(),
        _ => s.float_selection().map_or("-".to_owned(), |i| i.to_string()),
    }
}

/// One step of the script. The result is the change set (and the encode hashes of its jobs).
fn step(s: &mut Session, code: &str, cx: &mut Cx, fx: Fx, imgs: &[(String, Vec<u8>)]) -> Result<(ChangeSet, Vec<String>), String> {
    let f: Vec<&str> = code.split(':').collect();
    let nn = |i: usize| f[i].parse::<u32>().unwrap();
    let opt = |i: usize| (f[i] != "x").then(|| num(f[i]));
    let keys: Vec<String> = s.selection_pane_items().into_iter().map(|i| i.key).collect();
    let key = |i: usize| keys.get(f[i].parse::<usize>().unwrap()).cloned().unwrap_or_else(|| "nope".to_owned());
    let mut r = Run { s, cx, fx, encs: Vec::new() };
    let res = match f[0] {
        "ty" => r.exec(Command::Text(TextCmd::Type { text: Utf16::from_units(units(f[1])) })),
        "en" => r.exec(Command::Text(TextCmd::Enter)),
        "bs" => r.exec(Command::Text(TextCmd::Backspace)),
        "se" => {
            r.exec(Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(nn(1), nn(2)), focus: DocPosition::new(nn(3), nn(4)) }))
        }
        "un" => Ok(r.s.undo(&mut r.cx.cx())),
        "re" => Ok(r.s.redo(&mut r.cx.cx())),
        "so" => r.dr(DrawingCmd::SelectObject { index: Some(nn(1)) }),
        "si" => r.dr(DrawingCmd::SelectInline { para: nn(1), run: nn(2) }),
        "pi" => r.dr(DrawingCmd::SelectPaneItem { key: key(1).into(), add: f[2] == "1" }),
        "pm" => r.dr(DrawingCmd::SelectPaneItems { keys: vec![key(1).into(), key(2).into()] }),
        "pr" => r.dr(DrawingCmd::RenamePaneItem { key: key(1).into(), name: hstr(f[2]).into() }),
        "ph" => r.dr(DrawingCmd::SetPaneItemHidden { key: (f[1] != "-").then(|| key(1).into()), hidden: f[2] == "1" }),
        "sf" => {
            let fill = match nn(1) {
                0 => None,
                1 => Some(ShapeFill::Solid(color(1))),
                2 => Some(ShapeFill::Solid(color(0))),
                3 => Some(ShapeFill::Solid(color(2))),
                _ => Some(ShapeFill::Solid(PicColor {
                    kind: "schemeClr".into(),
                    val: "accent2".into(),
                    mods: Some(vec![("lumMod".into(), uo_docx::js::JsNumber::Num(50000.0))]),
                    ..PicColor::default()
                })),
            };
            r.dr(DrawingCmd::SetShapeFill { fill })
        }
        "sol" => {
            let c = match nn(1) {
                0 => DrawingCmd::SetShapeOutline { color: Some(Nullable::Val(color(0))), weight_pt: None, dash: None, compound: None },
                1 => DrawingCmd::SetShapeOutline { color: Some(Nullable::Null), weight_pt: None, dash: None, compound: None },
                2 => DrawingCmd::SetShapeOutline { color: None, weight_pt: Some(3.0), dash: Some("dash".into()), compound: None },
                _ => DrawingCmd::SetShapeOutline {
                    color: Some(Nullable::Val(color(2))),
                    weight_pt: Some(0.5),
                    dash: None,
                    compound: Some("dbl".into()),
                },
            };
            r.dr(c)
        }
        "sty" => r.dr(DrawingCmd::SetShapeStyle { n: num(f[1]) }),
        "chs" => r.dr(DrawingCmd::ChangeShape { prst: f[1].into() }),
        "tbo" => {
            let o = match nn(1) {
                0 => TextboxOptions { margin_l_pt: Some(10.0), margin_t_pt: Some(5.0), ..TextboxOptions::default() },
                1 => TextboxOptions { anchor: Some("ctr".into()), wrap: Some(false), ..TextboxOptions::default() },
                2 => TextboxOptions { direction: Some("vert".into()), autofit: Some("shape".into()), ..TextboxOptions::default() },
                _ => TextboxOptions {
                    anchor: Some("b".into()),
                    upright: Some(true),
                    autofit: Some("none".into()),
                    ..TextboxOptions::default()
                },
            };
            r.dr(DrawingCmd::SetTextboxOptions { o })
        }
        "sef" | "ef" => {
            let k = EFFECT_NAMES.iter().position(|n| *n == f[1]).unwrap();
            let preset = eff_preset(k, f[2]).map(Into::into);
            if f[0] == "sef" {
                r.dr(DrawingCmd::SetShapeEffect { kind: EFFECTS[k], preset })
            } else {
                r.pic(PictureCmd::SetEffect { kind: EFFECTS[k], preset })
            }
        }
        "dsh" => r.dr(DrawingCmd::DeleteSelectedShape),
        "itb" => {
            let page = r.fx.eo.iter().flatten().next().copied();
            r.dr(DrawingCmd::InsertTextbox { id: f[1].into(), page, theme_accent1: Some(f[2].into()), lang: "pt".into(), user: None })
        }
        "spf" => {
            let (ext, data) = &imgs[nn(1) as usize];
            r.dr(DrawingCmd::SetShapePictureFill { data: Arc::from(data.clone()), ext: ext.as_str().into(), tile: f[2] == "1" })
        }
        "est" => {
            let page = r.fx.sf.iter().flatten().next().map_or(0, |h| uo_jsnum::conv::to_uint32(h[4]));
            r.dr(DrawingCmd::EditSelectedTextbox { page })
        }
        "cdt" => {
            let rect = ObjRect { x: num(f[1]), y: num(f[2]), w: num(f[3]), h: num(f[4]) };
            let page = r.fx.eo.iter().flatten().next().copied().unwrap_or(0);
            let anchor_para = (f[6] != "x").then(|| f[6].parse::<u32>().unwrap());
            let res = r.dr(DrawingCmd::CreateDrawnTextbox { page, rect, vertical: f[5] == "1", anchor_para, lang: "pt".into() });
            let fr = r.fx.fr.iter().flatten().next().copied();
            match (res, fr) {
                (Ok(cs), Some(fr)) => {
                    let mut placed = r
                        .dr(DrawingCmd::PlaceAt { frame: fr, x: rect.x, y: rect.y })
                        .unwrap_or_else(|_| ChangeSet { events: Vec::new(), ..cs.clone() });
                    let mut events = cs.events.clone();
                    events.append(&mut placed.events);
                    placed.events = events;
                    Ok(placed)
                }
                (res, _) => res,
            }
        }
        "lk" => r.dr(DrawingCmd::LinkTextboxTo { key: key(1).into() }),
        "bl" => r.dr(DrawingCmd::BreakTextboxLink),
        "sdt" => r.dr(DrawingCmd::SetAsDefaultTextbox),
        "iw" => {
            let kind = WRAPS.iter().find(|w| w.0 == f[1]).unwrap().1;
            let res = r.pic(PictureCmd::SetWrap { kind });
            // inline -> floating: the TS read the box, refreshed, and placed the object
            let h = r.fx.sf.iter().flatten().next().copied();
            let fr = r.fx.fr.iter().flatten().next().copied();
            match (res, h, fr) {
                (Ok(cs), Some(h), Some(fr)) => {
                    let mut placed = r
                        .dr(DrawingCmd::PlaceAt { frame: fr, x: h[0], y: h[1] })
                        .unwrap_or_else(|_| ChangeSet { events: Vec::new(), ..cs.clone() });
                    // the events of both halves, in order
                    let mut events = cs.events.clone();
                    events.append(&mut placed.events);
                    placed.events = events;
                    Ok(placed)
                }
                (res, _, _) => res,
            }
        }
        "ip" => {
            let facts = r.fx.facts();
            r.pic(PictureCmd::SetPosition { preset: f[1].into(), facts })
        }
        "imw" => {
            let facts = r.fx.facts();
            r.pic(PictureCmd::SetMoveWithText { on: f[1] == "1", facts })
        }
        "isz" => r.pic(PictureCmd::SetSize { width_pt: opt(1), height_pt: opt(2), lock_aspect: (f[3] != "x").then(|| f[3] == "1") }),
        "ir" => r.pic(PictureCmd::SetRotation { deg: num(f[1]) }),
        "rt" => r.pic(PictureCmd::Rotate { deg: num(f[1]) }),
        "fl" => r.pic(PictureCmd::Flip { vertical: f[1] == "v" }),
        "ia" => r.pic(PictureCmd::SetAltText {
            text: Some(hstr(f[1]).into()),
            title: (f[2] != "x").then(|| hstr(f[2]).into()),
            decorative: (f[3] != "x").then(|| f[3] == "1"),
        }),
        "ar" => r.pic(PictureCmd::Arrange { op: ARRANGE.iter().find(|a| a.0 == f[1]).unwrap().1 }),
        "cff" => r.pic(PictureCmd::CropFillFit { fill: f[1] == "fill", natural_pt: opt(2).zip(opt(3)) }),
        "cta" => r.pic(PictureCmd::CropToAspect { w: num(f[1]), h: num(f[2]) }),
        "cts" => r.pic(PictureCmd::CropToShape { prst: f[1].into() }),
        "icr" => r.pic(PictureCmd::SetCrop { l: num(f[1]), t: num(f[2]), r: num(f[3]), b: num(f[4]) }),
        "ib" => {
            let c = match nn(1) {
                0 => PictureCmd::SetBorder { color: Some(Nullable::Val(color(0))), weight_pt: Some(2.0), dash: Some("dash".into()) },
                1 => PictureCmd::SetBorder { color: Some(Nullable::Null), weight_pt: None, dash: None },
                2 => PictureCmd::SetBorder { color: None, weight_pt: Some(4.0), dash: None },
                _ => PictureCmd::SetBorder { color: Some(Nullable::Val(color(2))), weight_pt: None, dash: None },
            };
            r.pic(c)
        }
        "it" => r.pic(PictureCmd::SetTransparency { pct: num(f[1]) }),
        "irc" => {
            let c = RECOLORS[nn(1) as usize];
            let arg = match c.strip_prefix('#') {
                Some(h) => RecolorArg::Color(PicColor { kind: "srgbClr".into(), val: h.into(), ..PicColor::default() }),
                None => RecolorArg::Preset(c.into()),
            };
            r.pic(PictureCmd::SetRecolor { arg })
        }
        "itc" => r.pic(PictureCmd::SetTransparentColor { hex: (f[1] != "-").then(|| f[1].into()) }),
        "ire" => r.pic(PictureCmd::Reset { size: f[1] == "1", natural_pt: opt(2).zip(opt(3)) }),
        "dim" => r.pic(PictureCmd::DeleteSelectedImage),
        "isl" => {
            let facts = r.fx.facts();
            let o = match nn(1) {
                0 => LayoutInput { wrap: Some(PicWrapKind::Square), width_pt: Some(120.0), ..LayoutInput::default() },
                1 => LayoutInput {
                    wrap: Some(PicWrapKind::TopAndBottom),
                    pos_h: Some(LayoutPos { relative_from: "margin".into(), align: Some("center".into()), offset_pt: None, pct: None }),
                    pos_v: Some(LayoutPos { relative_from: "paragraph".into(), align: None, offset_pt: Some(10.0), pct: None }),
                    ..LayoutInput::default()
                },
                2 => LayoutInput { wrap: Some(PicWrapKind::Inline), ..LayoutInput::default() },
                3 => LayoutInput {
                    width_pt: Some(80.0),
                    height_pt: Some(40.0),
                    lock_aspect: Some(false),
                    rotation: Some(20.0),
                    ..LayoutInput::default()
                },
                4 => LayoutInput {
                    pos_h: Some(LayoutPos { relative_from: "page".into(), align: None, offset_pt: Some(30.0), pct: None }),
                    pos_v: Some(LayoutPos { relative_from: "page".into(), align: None, offset_pt: Some(40.0), pct: None }),
                    wrap: Some(PicWrapKind::Front),
                    move_with_text: Some(false),
                    ..LayoutInput::default()
                },
                _ => LayoutInput {
                    wrap: Some(PicWrapKind::Through),
                    dist_pt: Some([Some(3.0), Some(4.0), Some(5.0), Some(6.0)]),
                    allow_overlap: Some(true),
                    layout_in_cell: Some(false),
                    ..LayoutInput::default()
                },
            };
            r.pic(PictureCmd::SetLayout { o: Box::new(o), facts })
        }
        "ins" => {
            // the decode of `insertImage` reads the size only (`bmp`); a later pixel read of the step is the paint of the page
            r.fx.gid.clear();
            let (ext, data) = &imgs[nn(1) as usize];
            r.pic(PictureCmd::Insert { data: Arc::from(data.clone()), ext: ext.as_str().into(), ole: None, max_width_twips: opt(2) })
        }
        "chg" => {
            r.fx.gid.clear();
            let (ext, data) = &imgs[nn(1) as usize];
            r.pic(PictureCmd::Change { data: Arc::from(data.clone()), ext: ext.as_str().into() })
        }
        "adj" => {
            let o = match nn(1) {
                0 => AdjustArg { bright: Some(0.2), ..AdjustArg::default() },
                1 => AdjustArg { contrast: Some(-0.3), saturation: Some(1.5), ..AdjustArg::default() },
                2 => AdjustArg { sharpen: Some(0.5), ..AdjustArg::default() },
                3 => AdjustArg { temperature: Some(5000.0), ..AdjustArg::default() },
                4 => AdjustArg {
                    bright: Some(0.0),
                    contrast: Some(0.0),
                    saturation: Some(1.0),
                    temperature: Some(6500.0),
                    ..AdjustArg::default()
                },
                _ => AdjustArg { bright: Some(-0.4), sharpen: Some(0.2), ..AdjustArg::default() },
            };
            r.pic(PictureCmd::SetAdjust { o })
        }
        "cmp" => r.pic(PictureCmd::Compress { ppi: num(f[1]), delete_cropped: f[2] == "1", all: f[3] == "1" }),
        "al" => {
            let kind = ALIGNS.iter().find(|a| a.0 == f[1]).unwrap().1;
            let rects = r.fx.rects();
            r.dr(DrawingCmd::AlignObjects { kind, rects })
        }
        "ial" => {
            let kind = ALIGNS.iter().find(|a| a.0 == f[1]).unwrap().1;
            let rects = r.fx.rects();
            r.pic(PictureCmd::Align { kind, relative_to: (f[2] != "x").then(|| f[2].into()), rects })
        }
        _ => panic!("step {code}"),
    };
    let encs = std::mem::take(&mut r.encs);
    res.map(|cs| (cs, encs))
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
        || p.props.indent_left_twips == Some(uo_docx::js::JsNumber::Num(360.0));
    match code {
        "en" if numbered && len == 0 => Some("D2b-F2 list rule: listEnterOnEmpty (D2a-T4)"),
        "bs" if list_like && sel.focus.offset == 0 => Some("D2b-F2 list rule: listBackspaceAtStart (D2a-T4)"),
        _ => None,
    }
}

#[test]
fn d2b_f7_session_matches_the_element() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2b-f7-session.capture")).unwrap();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let steps: usize = cap.meta("steps").unwrap().parse().unwrap();
    // the harness page replaced Math.sin/cos/... by the uo-jsnum functions: the capture says so
    assert!(cap.meta("math-override").is_some_and(|m| m.starts_with("uo-jsnum wasm")), "capture without the math override");
    let imgs = images(&cap);
    assert!(imgs.len() >= 5);
    let mut replay = Replay::new(&cap);
    for k in 0..imgs.len() {
        replay.skip(&format!("img#{k}"), "ext");
        replay.skip(&format!("img#{k}"), "b64");
    }
    let mut cx = Cx::new();
    let mut docs = 0;
    let mut ran = 0usize;
    for rec in cap.records().iter().filter(|r| r.key.ends_with("#0") && !r.key.starts_with("img#")) {
        let rel = rec.key.trim_end_matches("#0");
        let bytes = std::fs::read(root.join(rel)).unwrap();
        let env = SharedEnv::new();
        let mut s = Session::open(
            bytes,
            OpenOptions { sig_mode: SigMode::Ts, ts_formula_errors: true, ts_numbering: true, ..OpenOptions::default() },
            Box::new(env.clone()),
        )
        .unwrap();
        s.exec(Command::Settings(SettingsCmd::SetAuthor { name: "A".into() }), &mut cx.cx()).unwrap();
        let mut tb_before = false;
        for j in 0..=steps {
            let key = format!("{rel}#{j}");
            let Some(code) = cap.get(&key, "op") else { break };
            replay.check(&key, "op", code);
            // the editor's own uses of Math.random between the steps
            if j == 0 {
                env.advance_to(cap.get(&key, "rc").map_or(0, |v| v.parse().unwrap()));
            } else if let Some(prev) = cap.get(&format!("{rel}#{}", j - 1), "rc") {
                env.advance_to(prev.parse().unwrap());
            }
            // the D2b-F2 list rule covers only the step it applies to
            let pending = d2b_hook(&s, code);
            replay.skip(&key, "pane.text");
            replay.skip(&key, "bx.text");
            replay.skip(&key, "paras.trace");
            replay.skip(&key, "dx.full");
            replay.skip(&key, "st.text");
            let mut ev = None;
            let mut encs = Vec::new();
            if j == 0 {
                replay.skip(&key, "ev");
                replay.skip(&key, "fx");
            } else {
                tb_before = matches!(s.context(), EditContext::Textbox { .. });
                let fx = Fx::parse(cap.get(&key, "fx").unwrap_or("-"));
                match step(&mut s, code, &mut cx, fx, &imgs) {
                    Ok((cs, e)) => {
                        ev = Some(ev_text(&cs));
                        encs = e;
                    }
                    Err(e) => {
                        // a refusal the TS did not have is a mismatch: the facets show it
                        println!("{key}: {code}: {e}");
                        s.cancel_pending();
                    }
                }
                replay.skip(&key, "fx");
                ran += 1;
            }
            let mut facets = vec![
                ("paras", paras_digest(&s)),
                ("sel", sel_json(&s)),
                ("anc", anc_json(&s)),
                ("flags", flags(&s)),
                ("fs", fs_text(&s)),
                ("tb", tb_text(&s)),
                ("pane", sha(&pane_text(&s))),
                (
                    "st",
                    sha(&format!(
                        "{}#{}",
                        fmt_state(s.selected_object_state(false), false),
                        fmt_state(s.selected_object_state(true), true)
                    )),
                ),
            ];
            if let Some(ev) = ev {
                facets.push(("ev", ev));
            }
            if cap.get(&key, "docxml").is_some() {
                facets.push(("docxml", docxml(&mut s)));
            }
            if std::env::var("UO_F7_DIAG").is_ok() {
                let rust_st =
                    format!("{}#{}", fmt_state(s.selected_object_state(false), false), fmt_state(s.selected_object_state(true), true));
                let rust_pane = pane_text(&s).split('\n').collect::<Vec<_>>().join(" ~ ");
                for (f, mine, text) in [("st", rust_st, "st.text"), ("pane", rust_pane, "pane.text")] {
                    let want_sha = cap.get(&key, f).unwrap_or("");
                    let raw = if f == "pane" { pane_text(&s) } else { mine.clone() };
                    if sha(&raw) != want_sha {
                        println!("DIAG {key} [{code}] {f}\n  ts:   {}\n  rust: {mine}", cap.get(&key, text).unwrap_or("?"));
                    }
                }
            }
            if let Some(want) = cap.get(&key, "paras.trace") {
                let ts = String::from_utf8(b64(want)).unwrap_or_default();
                let doc = s.document();
                let mut paras = doc.body.paras.clone();
                let mut ids = doc.ids.clone();
                uo_docx::session::normalize::normalize(&mut paras, &mut ids);
                let mut sink = Sink(Stream::traced());
                for (i, p) in paras.iter().enumerate() {
                    sink.0.tag(b'E').u32(u32::try_from(i).unwrap());
                    let mut w = CanonWriter::new(&mut sink).with_media(&doc.media);
                    p.canon(&mut w);
                }
                let rust = sink.0.trace().to_owned();
                let (a, b): (Vec<&str>, Vec<&str>) = (ts.lines().collect(), rust.lines().collect());
                let at = a.iter().zip(b.iter()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
                println!("TRACE {key} [{code}]: first difference at line {at} of {} / {}", a.len(), b.len());
                let dec = |l: &str| -> String {
                    match l.strip_prefix("bytes ").and_then(|r| r.split_once(' ')) {
                        Some((_, hexs)) => {
                            let bytes: Vec<u8> =
                                (0..hexs.len() / 2).filter_map(|i| u8::from_str_radix(&hexs[i * 2..i * 2 + 2], 16).ok()).collect();
                            let units: Vec<u16> =
                                bytes.chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                            format!("str {:?}", String::from_utf16_lossy(&units))
                        }
                        None => l.to_owned(),
                    }
                };
                for k in at.saturating_sub(12)..(at + 60) {
                    let (x, y) = (dec(a.get(k).unwrap_or(&"<end>")), dec(b.get(k).unwrap_or(&"<end>")));
                    println!("  {}{x:<60} | {y}", if x == y { "  " } else { "!!" });
                }
            }
            if std::env::var("UO_F7_BX").is_ok()
                && cap.get(&key, "dx.full").is_some()
                && let Ok(bytes) = s.save_docx()
            {
                {
                    let pkg = uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
                    let t = pkg.part_text("word/document.xml").unwrap().map(|t| t.into_owned()).unwrap_or_default();
                    let ts = String::from_utf8(b64(cap.get(&key, "dx.full").unwrap_or(""))).unwrap_or_default();
                    let at = ts.bytes().zip(t.bytes()).position(|(a, b)| a != b).unwrap_or(ts.len().min(t.len()));
                    let win = |x: &str| x.get(at.saturating_sub(200)..(at + 300).min(x.len())).unwrap_or("?").to_owned();
                    if ts != t {
                        println!(
                            "DX {key} [{code}]: lengths ts {} rust {}, first difference at {at}\n  ts:   {}\n  rust: {}",
                            ts.len(),
                            t.len(),
                            win(&ts),
                            win(&t)
                        );
                    }
                }
            }
            if std::env::var("UO_F7_BX").is_ok() && code.starts_with("itb:") {
                let mut last = String::new();
                for p in s.document().body.paras.iter() {
                    for r in &p.runs {
                        if let Some(x) = r.ext().and_then(|e| e.textbox.as_deref()).and_then(|t| t.raw_drawing_xml.as_deref()) {
                            last = x.to_owned();
                        }
                    }
                }
                let ts = cap.get(&key, "bx.text").unwrap_or("").to_owned();
                let at = ts.chars().zip(last.chars()).position(|(a, b)| a != b);
                println!(
                    "BX {key} {code}: first difference at {at:?}\n  ts:   {}\n  rust: {}",
                    at.map_or(String::new(), |i| ts.chars().skip(i.saturating_sub(60)).take(220).collect()),
                    at.map_or(String::new(), |i| last.chars().skip(i.saturating_sub(60)).take(220).collect())
                );
            }
            if std::env::var("UO_F7_DUMP").is_ok_and(|d| d == key) {
                println!(
                    "RUST pane\n{}\nRUST st\n{} # {}",
                    pane_text(&s),
                    fmt_state(s.selected_object_state(false), false),
                    fmt_state(s.selected_object_state(true), true)
                );
            }
            replay.check(&key, "rc", &env.calls().to_string());
            // the count of a multi-selection is the handles the layout found (F7-D6)
            let multi = cap.get(&key, "mu").map_or(0, |v| v.parse::<usize>().unwrap());
            replay.skip(&key, "mu");
            if multi > 0 && j > 0 {
                replay.accept(
                    &key,
                    "cnt",
                    &s.selected_object_count().to_string(),
                    "F7-D6: selectedObjectCount counts the handles enumFloats finds on the laid-out pages (a multi-selection)",
                );
            } else {
                replay.check(&key, "cnt", &s.selected_object_count().to_string());
            }
            let enc_text = if encs.is_empty() { "-".to_owned() } else { encs.join(",") };
            if code.starts_with("cmp:") {
                if j > 0 {
                    replay.accept(&key, "enc", &enc_text, "F7-D1: compress resamples with an area average, the TS with drawImage smoothing (the bytes differ; the model is compared)");
                    replay.check(&key, "n", &s.compress_result().to_string());
                }
            } else if code.starts_with("adj:") {
                replay.check(&key, "enc", &enc_text);
            } else if j > 0 {
                // the other puts of the TS are the paint of the effects (a14, recolour): not a job
                replay.skip(&key, "enc");
            }
            if code.starts_with("itb:") && cap.get(&key, "n").is_some() {
                replay.skip(&key, "n");
            }
            if code == "est" {
                // `editSelectedTextbox()` returns `!!tbEdit` after it entered (false at once inside a box: no `floatSel`)
                replay.check(&key, "n", &(!tb_before && matches!(s.context(), EditContext::Textbox { .. })).to_string());
            }
            if j == 0 {
                replay.skip(&key, "enc");
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
    println!("d2b-f7-session: {docs} documents, {ran} steps, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_D2A_ALL").map_or(40, |_| 100_000)));
    }
    report.assert_clean();
}
