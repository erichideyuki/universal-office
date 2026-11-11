//! The F7 (drawing objects, text boxes, pictures) replay of the wasm module: the step mapping and the facets, COPIED from
//! `crates/uo-docx/tests/parity_d2b_f7_session.rs` (declared: one copy of test code, nothing here is shipped; a change of the
//! native replay's mapping or facets must be repeated here). Copied verbatim: `EFFECTS`, `EFFECT_NAMES`, `PRESET_IDS`, `RECOLORS`,
//! `ARRANGE`, `ALIGNS`, `WRAPS`, `color`, `eff_preset`, the `Fx` parse (of the facts the commands take: `sf`, `sfs`, `fr`, `eo`),
//! `fmt_state`, `pane_text`, `fs_text`, `sel_json`, `tb_text`, `flags`. Rewritten as "action k of a code" (the mapping inside the
//! test's `step`): `commands`. What the native `Run` does with the jobs (the image decodes and encodes answered from the fact log
//! `fx`, the `enc` hashes) is the JS side's (`session_replay.test.mjs`); the capture's images (`img#k`) are handed over once
//! (`uo_session_test_f7_image`). The native `SharedEnv::advance_to` (the editor's own uses of `Math.random` between the steps)
//! is `advance`: the session's environment is advanced through `Session::env_mut` until the test-only draw counter (`env::draws`,
//! cumulative since the open: `uo_session_test_f7_reset` before it) equals the capture's `rc`.
#![allow(dead_code, unused_imports, clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::all)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use uo_docx::js::Nullable;
use uo_docx::model::picture::PicColor;
use uo_docx::session::ops::context::EditContext;
use uo_docx::session::ops::drawing::{AlignKind, DrawingCmd, ObjFacts, ObjFrame, ObjRect, PaneKind, TextboxOptions};
use uo_docx::session::ops::pictures::{AdjustArg, ArrangeOp, EffectKind, LayoutInput, LayoutPos, PictureCmd, RecolorArg};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, DocPosition, Session};
use uo_docx::text::Utf16;
use uo_docx::write::emit::fx::PicWrapKind;
use uo_docx::write::emit::object::ShapeFill;
use uo_testkit::Digest;

use super::{Action, anc_json, cmd, docxml, paras_digest};

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

fn num(s: &str) -> f64 {
    uo_jsnum::parse::to_number(s)
}

fn js(x: f64) -> String {
    uo_jsnum::format::to_js_string(x)
}

fn sha(t: &str) -> String {
    Digest::of(t.as_bytes()).short()
}

fn hx(u: &[u16]) -> String {
    u.iter().map(|c| format!("{c:x}")).collect::<Vec<_>>().join(".")
}

fn hs(s: &str) -> String {
    hx(&s.encode_utf16().collect::<Vec<_>>())
}

fn hstr(hex: &str) -> String {
    String::from_utf16_lossy(&super::units(hex))
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

fn flags(s: &Session) -> String {
    format!("{}{}{}:{}", u8::from(s.is_dirty()), u8::from(s.can_undo()), u8::from(s.can_redo()), s.edits_since_load())
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
}

impl Fx {
    /// The facts of the step's `fx` field; the entries of the jobs (`bmp`, `gid`, `blob`) are the JS side's.
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
                "eo" => fx.eo.push((f[1] != "null").then(|| {
                    let v = num(f[5]);
                    if v >= 0.0 { uo_jsnum::conv::to_uint32(v) } else { 0 }
                })),
                _ => {}
            }
        }
        fx
    }

    fn rect(a: [f64; 6]) -> ObjRect {
        ObjRect { x: a[0], y: a[1], w: a[2], h: a[3] }
    }

    fn facts(&self) -> ObjFacts {
        ObjFacts { rect: self.sf.iter().flatten().next().map(|a| Fx::rect(*a)), frame: self.fr.iter().flatten().next().copied() }
    }

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

// ── the wasm wrappers (rewritten from the test's loop, not copied) ──

/// What a step does, in order.
enum Step {
    Cmd(Command),
    Undo,
    Redo,
}

#[derive(Default)]
struct Hs {
    fx: Fx,
    tb_before: bool,
}

thread_local! {
    static HS: RefCell<HashMap<usize, Hs>> = RefCell::new(HashMap::new());
    /// the capture's images (`img#k`): the extension and the bytes
    static IMAGES: RefCell<Vec<(String, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn forget(h: usize) {
    HS.with(|m| m.borrow_mut().remove(&h));
}

pub(super) fn set_image(k: usize, ext: String, data: Vec<u8>) {
    IMAGES.with(|v| {
        let mut v = v.borrow_mut();
        if v.len() <= k {
            v.resize(k + 1, (String::new(), Vec::new()));
        }
        v[k] = (ext, data);
    });
}

/// The step's inputs: the fact log, and the session's environment advanced to `target` random values (the cumulative count since
/// the open; `None` = no advance).
pub(super) fn begin(h: usize, s: &mut Session, fx: &str, target: Option<u32>) {
    if let Some(t) = target {
        while crate::session::env::draws() < t {
            s.env_mut().random_u32();
        }
    }
    let tb_before = matches!(s.context(), EditContext::Textbox { .. });
    HS.with(|m| {
        m.borrow_mut().insert(h, Hs { fx: Fx::parse(fx), tb_before });
    });
}

/// Action `k` of an F7 `code`; `None` when the code has no more. The second action of `iw` and `cdt` (`PlaceAt`) exists only when
/// the TS read the frame; the JS side stops the step when the first action was refused (the native `res` match).
pub(super) fn action(h: usize, s: &Session, code: &str, k: u32) -> Option<Action> {
    let step = HS.with(|m| {
        let m = m.borrow();
        let hs = m.get(&h)?;
        commands(s, code, &hs.fx, k)
    })?;
    match step {
        Step::Undo => Some(Action::Exec(super::w::ExecOp::Undo)),
        Step::Redo => Some(Action::Exec(super::w::ExecOp::Redo)),
        Step::Cmd(c) => cmd(c),
    }
}

fn commands(s: &Session, code: &str, fx: &Fx, k: u32) -> Option<Step> {
    let f: Vec<&str> = code.split(':').collect();
    let nn = |i: usize| f[i].parse::<u32>().unwrap();
    let opt = |i: usize| (f[i] != "x").then(|| num(f[i]));
    let keys: Vec<String> = s.selection_pane_items().into_iter().map(|i| i.key).collect();
    let key = |i: usize| keys.get(f[i].parse::<usize>().unwrap()).cloned().unwrap_or_else(|| "nope".to_owned());
    let imgs = |i: u32| IMAGES.with(|v| v.borrow().get(i as usize).cloned().unwrap());
    let d = |c: DrawingCmd| Some(Step::Cmd(Command::Drawing(c)));
    let p = |c: PictureCmd| Some(Step::Cmd(Command::Picture(c)));
    let t = |c: TextCmd| Some(Step::Cmd(Command::Text(c)));
    if k > 0 {
        return match f[0] {
            "cdt" if k == 1 => {
                let fr = fx.fr.iter().flatten().next().copied()?;
                let (x, y) = (num(f[1]), num(f[2]));
                d(DrawingCmd::PlaceAt { frame: fr, x, y })
            }
            "iw" if k == 1 => {
                let h = fx.sf.iter().flatten().next().copied()?;
                let fr = fx.fr.iter().flatten().next().copied()?;
                d(DrawingCmd::PlaceAt { frame: fr, x: h[0], y: h[1] })
            }
            _ => None,
        };
    }
    match f[0] {
        "ty" => t(TextCmd::Type { text: Utf16::from_units(super::units(f[1])) }),
        "en" => t(TextCmd::Enter),
        "bs" => t(TextCmd::Backspace),
        "se" => t(TextCmd::SetSelection { anchor: DocPosition::new(nn(1), nn(2)), focus: DocPosition::new(nn(3), nn(4)) }),
        "un" => Some(Step::Undo),
        "re" => Some(Step::Redo),
        "so" => d(DrawingCmd::SelectObject { index: Some(nn(1)) }),
        "si" => d(DrawingCmd::SelectInline { para: nn(1), run: nn(2) }),
        "pi" => d(DrawingCmd::SelectPaneItem { key: key(1).into(), add: f[2] == "1" }),
        "pm" => d(DrawingCmd::SelectPaneItems { keys: vec![key(1).into(), key(2).into()] }),
        "pr" => d(DrawingCmd::RenamePaneItem { key: key(1).into(), name: hstr(f[2]).into() }),
        "ph" => d(DrawingCmd::SetPaneItemHidden { key: (f[1] != "-").then(|| key(1).into()), hidden: f[2] == "1" }),
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
            d(DrawingCmd::SetShapeFill { fill })
        }
        "sol" => d(match nn(1) {
            0 => DrawingCmd::SetShapeOutline { color: Some(Nullable::Val(color(0))), weight_pt: None, dash: None, compound: None },
            1 => DrawingCmd::SetShapeOutline { color: Some(Nullable::Null), weight_pt: None, dash: None, compound: None },
            2 => DrawingCmd::SetShapeOutline { color: None, weight_pt: Some(3.0), dash: Some("dash".into()), compound: None },
            _ => DrawingCmd::SetShapeOutline {
                color: Some(Nullable::Val(color(2))),
                weight_pt: Some(0.5),
                dash: None,
                compound: Some("dbl".into()),
            },
        }),
        "sty" => d(DrawingCmd::SetShapeStyle { n: num(f[1]) }),
        "chs" => d(DrawingCmd::ChangeShape { prst: f[1].into() }),
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
            d(DrawingCmd::SetTextboxOptions { o })
        }
        "sef" | "ef" => {
            let kd = EFFECT_NAMES.iter().position(|n| *n == f[1]).unwrap();
            let preset = eff_preset(kd, f[2]).map(Into::into);
            if f[0] == "sef" {
                d(DrawingCmd::SetShapeEffect { kind: EFFECTS[kd], preset })
            } else {
                p(PictureCmd::SetEffect { kind: EFFECTS[kd], preset })
            }
        }
        "dsh" => d(DrawingCmd::DeleteSelectedShape),
        "itb" => {
            let page = fx.eo.iter().flatten().next().copied();
            d(DrawingCmd::InsertTextbox { id: f[1].into(), page, theme_accent1: Some(f[2].into()), lang: "pt".into(), user: None })
        }
        "spf" => {
            let (ext, data) = imgs(nn(1));
            d(DrawingCmd::SetShapePictureFill { data: Arc::from(data), ext: ext.as_str().into(), tile: f[2] == "1" })
        }
        "est" => {
            let page = fx.sf.iter().flatten().next().map_or(0, |h| uo_jsnum::conv::to_uint32(h[4]));
            d(DrawingCmd::EditSelectedTextbox { page })
        }
        "cdt" => {
            let rect = ObjRect { x: num(f[1]), y: num(f[2]), w: num(f[3]), h: num(f[4]) };
            let page = fx.eo.iter().flatten().next().copied().unwrap_or(0);
            let anchor_para = (f[6] != "x").then(|| f[6].parse::<u32>().unwrap());
            d(DrawingCmd::CreateDrawnTextbox { page, rect, vertical: f[5] == "1", anchor_para, lang: "pt".into() })
        }
        "lk" => d(DrawingCmd::LinkTextboxTo { key: key(1).into() }),
        "bl" => d(DrawingCmd::BreakTextboxLink),
        "sdt" => d(DrawingCmd::SetAsDefaultTextbox),
        "iw" => {
            let kind = WRAPS.iter().find(|w| w.0 == f[1]).unwrap().1;
            p(PictureCmd::SetWrap { kind })
        }
        "ip" => p(PictureCmd::SetPosition { preset: f[1].into(), facts: fx.facts() }),
        "imw" => p(PictureCmd::SetMoveWithText { on: f[1] == "1", facts: fx.facts() }),
        "isz" => p(PictureCmd::SetSize { width_pt: opt(1), height_pt: opt(2), lock_aspect: (f[3] != "x").then(|| f[3] == "1") }),
        "ir" => p(PictureCmd::SetRotation { deg: num(f[1]) }),
        "rt" => p(PictureCmd::Rotate { deg: num(f[1]) }),
        "fl" => p(PictureCmd::Flip { vertical: f[1] == "v" }),
        "ia" => p(PictureCmd::SetAltText {
            text: Some(hstr(f[1]).into()),
            title: (f[2] != "x").then(|| hstr(f[2]).into()),
            decorative: (f[3] != "x").then(|| f[3] == "1"),
        }),
        "ar" => p(PictureCmd::Arrange { op: ARRANGE.iter().find(|a| a.0 == f[1]).unwrap().1 }),
        "cff" => p(PictureCmd::CropFillFit { fill: f[1] == "fill", natural_pt: opt(2).zip(opt(3)) }),
        "cta" => p(PictureCmd::CropToAspect { w: num(f[1]), h: num(f[2]) }),
        "cts" => p(PictureCmd::CropToShape { prst: f[1].into() }),
        "icr" => p(PictureCmd::SetCrop { l: num(f[1]), t: num(f[2]), r: num(f[3]), b: num(f[4]) }),
        "ib" => p(match nn(1) {
            0 => PictureCmd::SetBorder { color: Some(Nullable::Val(color(0))), weight_pt: Some(2.0), dash: Some("dash".into()) },
            1 => PictureCmd::SetBorder { color: Some(Nullable::Null), weight_pt: None, dash: None },
            2 => PictureCmd::SetBorder { color: None, weight_pt: Some(4.0), dash: None },
            _ => PictureCmd::SetBorder { color: Some(Nullable::Val(color(2))), weight_pt: None, dash: None },
        }),
        "it" => p(PictureCmd::SetTransparency { pct: num(f[1]) }),
        "irc" => {
            let c = RECOLORS[nn(1) as usize];
            let arg = match c.strip_prefix('#') {
                Some(h) => RecolorArg::Color(PicColor { kind: "srgbClr".into(), val: h.into(), ..PicColor::default() }),
                None => RecolorArg::Preset(c.into()),
            };
            p(PictureCmd::SetRecolor { arg })
        }
        "itc" => p(PictureCmd::SetTransparentColor { hex: (f[1] != "-").then(|| f[1].into()) }),
        "ire" => p(PictureCmd::Reset { size: f[1] == "1", natural_pt: opt(2).zip(opt(3)) }),
        "dim" => p(PictureCmd::DeleteSelectedImage),
        "isl" => {
            let facts = fx.facts();
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
            p(PictureCmd::SetLayout { o: Box::new(o), facts })
        }
        "ins" => {
            let (ext, data) = imgs(nn(1));
            p(PictureCmd::Insert { data: Arc::from(data), ext: ext.as_str().into(), ole: None, max_width_twips: opt(2) })
        }
        "chg" => {
            let (ext, data) = imgs(nn(1));
            p(PictureCmd::Change { data: Arc::from(data), ext: ext.as_str().into() })
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
            p(PictureCmd::SetAdjust { o })
        }
        "cmp" => p(PictureCmd::Compress { ppi: num(f[1]), delete_cropped: f[2] == "1", all: f[3] == "1" }),
        "al" => {
            let kind = ALIGNS.iter().find(|a| a.0 == f[1]).unwrap().1;
            d(DrawingCmd::AlignObjects { kind, rects: fx.rects() })
        }
        "ial" => {
            let kind = ALIGNS.iter().find(|a| a.0 == f[1]).unwrap().1;
            p(PictureCmd::Align { kind, relative_to: (f[2] != "x").then(|| f[2].into()), rects: fx.rects() })
        }
        _ => None,
    }
}

/// `paras`, `sel`, `anc`, `flags`, `fs`, `tb`, `pane`, `st`, `cnt`, `n` (`cmp`, `est`), `rc` and `docxml`.
pub(super) fn facets(h: usize, s: &mut Session, code: &str, want_docxml: bool) -> Vec<(&'static str, String)> {
    let tb_before = HS.with(|m| m.borrow().get(&h).is_some_and(|x| x.tb_before));
    let mut v = vec![
        ("paras", paras_digest(s)),
        ("sel", sel_json(s)),
        ("anc", anc_json(s)),
        ("flags", flags(s)),
        ("fs", fs_text(s)),
        ("tb", tb_text(s)),
        ("pane", sha(&pane_text(s))),
        ("st", sha(&format!("{}#{}", fmt_state(s.selected_object_state(false), false), fmt_state(s.selected_object_state(true), true)))),
        ("cnt", s.selected_object_count().to_string()),
        ("rc", crate::session::env::draws().to_string()),
    ];
    if code.starts_with("cmp:") {
        v.push(("n", s.compress_result().to_string()));
    } else if code == "est" {
        v.push(("n", (!tb_before && matches!(s.context(), EditContext::Textbox { .. })).to_string()));
    }
    if want_docxml {
        v.push(("docxml", docxml(s)));
    }
    v
}
