//! Shared helpers of the D2b-F7 memory, no-panic and behaviour tests: the picture used by the
//! constructed documents, a fake shell that answers the decode/encode jobs of the two-phase
//! commands (plausible and hostile), and seeded command generators for the drawing and picture
//! families.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use uo_docx::ids::HfPartId;
use uo_docx::js::Nullable;
use uo_docx::model::picture::PicColor;
use uo_docx::session::ops::context::ContextCmd;
use uo_docx::session::ops::drawing::{AlignKind, DrawingCmd, GalleryBox, ObjFacts, ObjFrame, ObjRect, TextboxOptions};
use uo_docx::session::ops::pictures::{AdjustArg, ArrangeOp, EffectKind, LayoutInput, LayoutPos, OleArg, PictureCmd, RecolorArg};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{CellHop, Command, DocPosition, Job, JobResult, Outcome, Session};
use uo_docx::text::Utf16;
use uo_docx::write::emit::fx::PicWrapKind;
use uo_docx::write::emit::object::ShapeFill;
use uo_testkit::generate::Rng;

use crate::d2a_support::{Cx, Step};

/// A 1 x 1 px PNG.
pub const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f,
    0x15, 0xc4, 0x89, 0, 0, 0, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0, 5, 0, 1, 0xff, 0x56, 0xc7,
    0x2f, 0x0d, 0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// A pixel pattern of `w` x `h`: opaque left half, transparent right half with a coloured rim.
pub fn pattern(w: u32, h: u32) -> Vec<u8> {
    let mut px = Vec::with_capacity((w as usize) * (h as usize) * 4);
    for y in 0..h {
        for x in 0..w {
            let a = if x * 2 < w {
                255
            } else if x == w - 1 || y == 0 {
                120
            } else {
                0
            };
            px.extend_from_slice(&[(x * 7 % 256) as u8, (y * 13 % 256) as u8, 90, a]);
        }
    }
    px
}

/// How a fake shell answers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// decodes with a size derived from the bytes, encodes to a small PNG, fails every 7th job
    Plain,
    /// also answers with sizes and pixel buffers that do not agree, and completes jobs twice
    Hostile,
}

/// Answers the jobs of `out` (and of the rounds after it) the way a shell would; returns the last
/// outcome (`None` when a completion was refused). A stuck command is cancelled after 6 rounds.
pub fn answer_jobs(s: &mut Session, cx: &mut Cx, mut out: Outcome, rng: &mut Rng, shell: Shell, counter: &mut u32) -> Option<Outcome> {
    for _ in 0..6 {
        let Outcome::NeedsJobs(jobs) = out else { return Some(out) };
        let mut last = None;
        for job in &jobs {
            *counter += 1;
            let fail = (*counter).is_multiple_of(7);
            let r = match job {
                Job::DecodeImage { bytes, .. } if !fail => {
                    let (w, h) = (1 + u32::try_from(bytes.len() % 37).unwrap(), 1 + u32::try_from((bytes.len() / 3) % 23).unwrap());
                    if shell == Shell::Hostile {
                        match rng.below(9) {
                            0 => JobResult::Image { width: 0, height: 0, rgba: None },
                            1 => JobResult::Image { width: 1, height: 100_000, rgba: None },
                            2 => JobResult::Image { width: w, height: h, rgba: Some(Arc::from(vec![0u8; 3])) },
                            3 => JobResult::Image { width: u32::MAX, height: u32::MAX, rgba: None },
                            4 => JobResult::Image { width: 0, height: h, rgba: Some(Arc::from(pattern(w, h))) },
                            5 => JobResult::Image { width: w, height: 0, rgba: Some(Arc::from(pattern(w, h))) },
                            6 => JobResult::Image { width: w, height: h, rgba: Some(Arc::from(vec![0u8; 5])) },
                            _ => JobResult::Image { width: w, height: h, rgba: Some(Arc::from(pattern(w, h))) },
                        }
                    } else {
                        JobResult::Image { width: w, height: h, rgba: Some(Arc::from(pattern(w, h))) }
                    }
                }
                Job::EncodeImage { .. } if !fail => JobResult::Bytes(Arc::from(PNG)),
                _ => JobResult::Failed,
            };
            last = s.complete(job.id(), r.clone(), &mut cx.cx()).ok();
            if shell == Shell::Hostile && rng.below(8) == 0 {
                // a job completed twice, one never issued
                let _ = s.complete(job.id(), r, &mut cx.cx());
                let _ = s.complete(uo_docx::session::JobId(u32::MAX), JobResult::Failed, &mut cx.cx());
            }
        }
        match last {
            Some(o) => out = o,
            None => {
                s.cancel_pending();
                return None;
            }
        }
    }
    s.cancel_pending();
    None
}

/// Runs one step the way the shell does: commands answer their jobs, undo/redo run.
pub fn run_step(s: &mut Session, cx: &mut Cx, step: Step, rng: &mut Rng, shell: Shell, counter: &mut u32) {
    match step {
        Step::Undo => {
            s.undo(&mut cx.cx());
        }
        Step::Redo => {
            s.redo(&mut cx.cx());
        }
        Step::Cmd(c) => {
            if let Ok(out) = s.exec(c, &mut cx.cx()) {
                let _ = answer_jobs(s, cx, out, rng, shell, counter);
            } else {
                // a refused command leaves nothing pending
                s.cancel_pending();
            }
        }
    }
}

fn pos(rng: &mut Rng, s: &Session) -> DocPosition {
    let paras = s.paras().unwrap();
    let para = rng.below(paras.len());
    let len = paras.get(para).map_or(0, uo_docx::session::text::para_length);
    DocPosition::new(u32::try_from(para).unwrap(), u32::try_from(rng.below(len + 1)).unwrap())
}

fn color(rng: &mut Rng) -> PicColor {
    match rng.below(3) {
        0 => PicColor { kind: "srgbClr".into(), val: ["FF0000", "00FF00", "336699"][rng.below(3)].into(), ..PicColor::default() },
        1 => PicColor { kind: "schemeClr".into(), val: ["accent1", "tx1", "bg1"][rng.below(3)].into(), ..PicColor::default() },
        _ => PicColor { kind: "prstClr".into(), val: "black".into(), ..PicColor::default() },
    }
}

fn frame() -> ObjFrame {
    ObjFrame {
        page_w: 12240.0,
        page_h: 15840.0,
        mar_l: 1440.0,
        mar_r: 1440.0,
        mar_t: 1440.0,
        mar_b: 1440.0,
        col_x: 1440.0,
        col_w: 9360.0,
        body_top: 1440.0,
        para_top: 1440.0,
    }
}

fn facts(rng: &mut Rng) -> ObjFacts {
    ObjFacts {
        rect: (rng.below(2) == 0).then(|| ObjRect {
            x: rng.below(8000) as f64,
            y: rng.below(10000) as f64,
            w: 500.0 + rng.below(3000) as f64,
            h: 500.0 + rng.below(3000) as f64,
        }),
        frame: (rng.below(2) == 0).then(frame),
    }
}

const WRAPS: [PicWrapKind; 7] = [
    PicWrapKind::Inline,
    PicWrapKind::Square,
    PicWrapKind::Tight,
    PicWrapKind::Through,
    PicWrapKind::TopAndBottom,
    PicWrapKind::Behind,
    PicWrapKind::Front,
];
const ARRANGE: [ArrangeOp; 6] =
    [ArrangeOp::Front, ArrangeOp::Back, ArrangeOp::Forward, ArrangeOp::Backward, ArrangeOp::FrontOfText, ArrangeOp::BehindText];
const EFFECTS: [EffectKind; 6] =
    [EffectKind::Shadow, EffectKind::Reflection, EffectKind::Glow, EffectKind::SoftEdge, EffectKind::Bevel, EffectKind::Rotation];
const PRESETS: [&str; 10] = ["outer-br", "outer-b", "glow-0-1", "glow-2-3", "10", "circle", "half", "grayscale", "washout", "nope"];
const TEMPLATES: [&str; 7] = ["simple", "quote", "sidebarRight", "sidebarLeft", "band", "note", "nope"];
const SHAPES: [&str; 6] = ["rect", "ellipse", "triangle", "star5", "roundRect", "nope"];
const ASPECTS: [(f64, f64); 4] = [(1.0, 1.0), (16.0, 9.0), (4.0, 3.0), (3.0, 4.0)];

/// A picture of a few bytes: its length is what the fake shell derives the size from.
fn picture_bytes(rng: &mut Rng) -> Arc<[u8]> {
    let mut v = PNG.to_vec();
    v.extend(std::iter::repeat_n(0u8, rng.below(30)));
    Arc::from(v)
}

/// A step an editor user could do now: selects objects, formats them, inserts pictures and boxes,
/// types, undoes.
pub fn f7_step(rng: &mut Rng, s: &Session) -> Step {
    let k = rng.below(100);
    let n = u32::try_from(s.pane_keys().len()).unwrap();
    let b = |rng: &mut Rng| rng.below(2) == 0;
    let cmd = match k {
        0..=7 => return Step::Cmd(Command::Text(TextCmd::Type { text: Utf16::from(["a", "Olá ", "😀", "x y"][rng.below(4)]) })),
        8..=9 => return Step::Cmd(Command::Text(TextCmd::Enter)),
        10..=12 => return Step::Cmd(Command::Text(TextCmd::Backspace)),
        13..=16 => {
            let a = pos(rng, s);
            return Step::Cmd(Command::Text(TextCmd::SetSelection { anchor: a, focus: a }));
        }
        17..=19 => return Step::Undo,
        20..=21 => return Step::Redo,
        22..=29 => Command::Drawing(DrawingCmd::SelectObject {
            index: if rng.below(6) == 0 { None } else { Some(u32::try_from(rng.below(usize::try_from(n).unwrap() + 2)).unwrap()) },
        }),
        30..=33 => {
            let para = u32::try_from(rng.below(s.paras().unwrap().len())).unwrap();
            Command::Drawing(DrawingCmd::SelectInline { para, run: u32::try_from(rng.below(4)).unwrap() })
        }
        34..=38 => Command::Picture(PictureCmd::Insert {
            data: picture_bytes(rng),
            ext: ["png", "jpg", "gif"][rng.below(3)].into(),
            ole: None,
            max_width_twips: (b(rng)).then_some(6000.0),
        }),
        39..=42 => Command::Picture(PictureCmd::SetWrap { kind: WRAPS[rng.below(7)] }),
        43..=44 => Command::Picture(PictureCmd::SetPosition {
            preset: ["top-right", "center", "bottom-left", "middle-center"][rng.below(4)].into(),
            facts: facts(rng),
        }),
        45 => Command::Picture(PictureCmd::SetMoveWithText { on: b(rng), facts: facts(rng) }),
        46 => Command::Picture(PictureCmd::SetLockAnchor { on: b(rng) }),
        47 => Command::Picture(PictureCmd::SetSize {
            width_pt: Some(20.0 + rng.below(300) as f64),
            height_pt: b(rng).then_some(30.0 + rng.below(200) as f64),
            lock_aspect: Some(b(rng)),
        }),
        48 => Command::Picture(PictureCmd::SetRotation { deg: rng.below(360) as f64 }),
        49 => Command::Picture(PictureCmd::Rotate { deg: [90.0, -90.0, 15.0][rng.below(3)] }),
        50 => Command::Picture(PictureCmd::Flip { vertical: b(rng) }),
        51 => Command::Picture(PictureCmd::SetAltText {
            text: Some("alt".into()),
            title: b(rng).then(|| "t".into()),
            decorative: b(rng).then(|| b(rng)),
        }),
        52..=53 => Command::Picture(PictureCmd::Arrange { op: ARRANGE[rng.below(6)] }),
        54 => Command::Picture(PictureCmd::CropFillFit { fill: b(rng), natural_pt: Some((100.0, 50.0)) }),
        55 => {
            let (w, h) = ASPECTS[rng.below(4)];
            Command::Picture(PictureCmd::CropToAspect { w, h })
        }
        56 => Command::Picture(PictureCmd::CropToShape { prst: SHAPES[rng.below(6)].into() }),
        57 => Command::Picture(PictureCmd::SetCrop {
            l: rng.below(30) as f64 / 100.0,
            t: rng.below(30) as f64 / 100.0,
            r: rng.below(30) as f64 / 100.0,
            b: rng.below(30) as f64 / 100.0,
        }),
        58 => Command::Picture(PictureCmd::SetBorder {
            color: Some(Nullable::Val(color(rng))),
            weight_pt: Some(0.5 + rng.below(6) as f64),
            dash: b(rng).then(|| "dash".into()),
        }),
        59..=60 => {
            Command::Picture(PictureCmd::SetEffect { kind: EFFECTS[rng.below(6)], preset: b(rng).then(|| PRESETS[rng.below(10)].into()) })
        }
        61 => Command::Picture(PictureCmd::SetTransparency { pct: rng.below(100) as f64 }),
        62 => Command::Picture(PictureCmd::SetRecolor {
            arg: if b(rng) { RecolorArg::Preset(PRESETS[rng.below(10)].into()) } else { RecolorArg::Color(color(rng)) },
        }),
        63 => Command::Picture(PictureCmd::SetTransparentColor { hex: b(rng).then(|| "FF0000".into()) }),
        64 => Command::Picture(PictureCmd::Reset { size: b(rng), natural_pt: Some((100.0, 50.0)) }),
        65 => Command::Picture(PictureCmd::DeleteSelectedImage),
        66 => Command::Picture(PictureCmd::SetLayout {
            o: Box::new(LayoutInput {
                wrap: Some(WRAPS[rng.below(7)]),
                width_pt: b(rng).then(|| 50.0 + rng.below(200) as f64),
                pos_h: b(rng).then(|| LayoutPos {
                    relative_from: "margin".into(),
                    align: Some("center".into()),
                    offset_pt: None,
                    pct: None,
                }),
                rotation: b(rng).then(|| rng.below(90) as f64),
                ..LayoutInput::default()
            }),
            facts: facts(rng),
        }),
        67..=68 => Command::Picture(PictureCmd::SetAdjust {
            o: AdjustArg {
                bright: Some(rng.below(100) as f64 - 50.0),
                contrast: b(rng).then(|| rng.below(100) as f64 - 50.0),
                ..AdjustArg::default()
            },
        }),
        69 => Command::Picture(PictureCmd::Change { data: picture_bytes(rng), ext: "png".into() }),
        70..=71 => Command::Picture(PictureCmd::Compress { ppi: [96.0, 150.0, 220.0][rng.below(3)], delete_cropped: b(rng), all: b(rng) }),
        72 => Command::Drawing(DrawingCmd::SetShapeFill { fill: if b(rng) { Some(ShapeFill::Solid(color(rng))) } else { None } }),
        73 => Command::Drawing(DrawingCmd::SetShapeOutline {
            color: b(rng).then(|| Nullable::Val(color(rng))),
            weight_pt: b(rng).then(|| rng.below(8) as f64),
            dash: b(rng).then(|| "dot".into()),
            compound: None,
        }),
        74 => Command::Drawing(DrawingCmd::SetShapeStyle { n: rng.below(80) as f64 }),
        75 => Command::Drawing(DrawingCmd::ChangeShape { prst: SHAPES[rng.below(6)].into() }),
        76 => Command::Drawing(DrawingCmd::SetTextboxOptions {
            o: TextboxOptions {
                margin_l_pt: Some(rng.below(20) as f64),
                anchor: Some(["t", "ctr", "b"][rng.below(3)].into()),
                wrap: b(rng).then(|| b(rng)),
                ..TextboxOptions::default()
            },
        }),
        77 => Command::Drawing(DrawingCmd::SetShapeEffect {
            kind: EFFECTS[rng.below(6)],
            preset: b(rng).then(|| PRESETS[rng.below(10)].into()),
        }),
        78 => Command::Drawing(DrawingCmd::DeleteSelectedShape),
        79..=82 => Command::Drawing(DrawingCmd::InsertTextbox {
            id: TEMPLATES[rng.below(7)].into(),
            page: b(rng).then_some(0),
            theme_accent1: None,
            lang: ["en", "pt", "es"][rng.below(3)].into(),
            user: None,
        }),
        83 => Command::Drawing(DrawingCmd::LinkTextboxTo {
            key: s.pane_keys().get(rng.below(usize::try_from(n).unwrap() + 1)).cloned().unwrap_or_default().into(),
        }),
        84 => Command::Drawing(DrawingCmd::BreakTextboxLink),
        85 => Command::Drawing(DrawingCmd::SetAsDefaultTextbox),
        86 => Command::Drawing(DrawingCmd::AlignObjects {
            kind: [
                AlignKind::Left,
                AlignKind::Center,
                AlignKind::Right,
                AlignKind::Top,
                AlignKind::Middle,
                AlignKind::Bottom,
                AlignKind::DistributeH,
                AlignKind::DistributeV,
            ][rng.below(8)],
            rects: vec![facts(rng), facts(rng), facts(rng)],
        }),
        87 => Command::Drawing(DrawingCmd::PlaceAt { frame: frame(), x: rng.below(8000) as f64, y: rng.below(8000) as f64 }),
        88 => Command::Drawing(DrawingCmd::CreateDrawnTextbox {
            page: 0,
            rect: ObjRect { x: 500.0, y: 500.0, w: 2000.0, h: 900.0 },
            vertical: b(rng),
            anchor_para: Some(0),
            lang: "pt".into(),
        }),
        89 => {
            let keys = s.pane_keys();
            let key = keys.get(rng.below(keys.len() + 1)).cloned().unwrap_or_else(|| "nope".to_owned());
            match rng.below(4) {
                0 => Command::Drawing(DrawingCmd::SelectPaneItem { key: key.into(), add: b(rng) }),
                1 => Command::Drawing(DrawingCmd::RenamePaneItem { key: key.into(), name: "Nome".into() }),
                2 => Command::Drawing(DrawingCmd::SetPaneItemHidden { key: b(rng).then(|| key.into()), hidden: b(rng) }),
                _ => Command::Drawing(DrawingCmd::SelectPaneItems { keys: keys.iter().take(2).map(|k| k.clone().into()).collect() }),
            }
        }
        90 => Command::Settings(SettingsCmd::SetTracking { on: b(rng) }),
        _ => return Step::Cmd(ctx_cmd(rng, s)),
    };
    Step::Cmd(cmd)
}

/// Enter or leave a cell, a note or a header/footer part (the picture commands work in any context).
fn ctx_cmd(rng: &mut Rng, s: &Session) -> Command {
    match rng.below(4) {
        0 => {
            let n = s.document().body.tables.len();
            if n > 0 {
                let anchor = u32::try_from(rng.below(n)).unwrap();
                Command::Context(ContextCmd::EnterCell { anchor, hops: vec![CellHop { row: 0, col: 0, block: None }], caret: None })
            } else {
                Command::Context(ContextCmd::ExitCell)
            }
        }
        1 => {
            let n = s.document().hf.len();
            if n > 0 {
                Command::Context(ContextCmd::EnterHeaderFooter { part: HfPartId(u32::try_from(rng.below(n)).unwrap()), page: 0 })
            } else {
                Command::Context(ContextCmd::ExitHeaderFooter)
            }
        }
        _ => Command::Context([ContextCmd::ExitCell, ContextCmd::ExitNote, ContextCmd::ExitHeaderFooter][rng.below(3)].clone()),
    }
}

fn wild_u32(rng: &mut Rng) -> u32 {
    match rng.below(4) {
        0 => rng.next_u32(),
        1 => u32::MAX - u32::try_from(rng.below(3)).unwrap(),
        _ => u32::try_from(rng.below(40)).unwrap(),
    }
}

fn wild_f64(rng: &mut Rng) -> f64 {
    [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0, 1e300, -1e300, 0.0, 1e-300, 4.2e9, -1.0, 0.5, 100.0][rng.below(12)]
}

/// Names, alt texts and keys: the XML specials the writers escape are in.
fn wild_name(rng: &mut Rng) -> String {
    match rng.below(3) {
        0 => "𝒜b<>&\"'".repeat(5),
        _ => wild_text(rng),
    }
}

fn wild_text(rng: &mut Rng) -> String {
    match rng.below(6) {
        0 => String::new(),
        1 => "x".repeat(100 + rng.below(50)),
        2 => "\u{0}\u{fffd}".to_owned(),
        3 => "𝒜b é".repeat(5),
        4 => "glow-99999999999-9".to_owned(),
        _ => "nope".to_owned(),
    }
}

/// A step with hostile arguments: NaN/inf/-0/1e300 sizes, rotations, crops, ppi and style numbers,
/// out-of-range indices, unknown ids and keys, garbage image bytes, odd selections.
pub fn hostile_step(rng: &mut Rng, s: &Session) -> Step {
    if rng.below(100) < 35 {
        return f7_step(rng, s);
    }
    let w = |rng: &mut Rng| wild_f64(rng);
    let opt = |rng: &mut Rng| (rng.below(2) == 0).then(|| wild_f64(rng));
    let bytes = |rng: &mut Rng| -> Arc<[u8]> {
        match rng.below(4) {
            0 => Arc::from(Vec::new()),
            1 => Arc::from(&b"junk"[..]),
            2 => Arc::from(vec![0xffu8; 40]),
            _ => picture_bytes(rng),
        }
    };
    let ff = || ObjFacts {
        rect: Some(ObjRect { x: f64::NAN, y: f64::INFINITY, w: -1.0, h: 1e300 }),
        frame: Some(ObjFrame { page_w: f64::NAN, ..ObjFrame::default() }),
    };
    let cmd = match rng.below(40) {
        0 => Command::Drawing(DrawingCmd::SelectObject { index: Some(wild_u32(rng)) }),
        1 => Command::Drawing(DrawingCmd::SelectInline { para: wild_u32(rng), run: wild_u32(rng) }),
        2 => Command::Drawing(DrawingCmd::SelectPaneItem { key: wild_text(rng).into(), add: rng.below(2) == 0 }),
        3 => Command::Drawing(DrawingCmd::RenamePaneItem { key: wild_text(rng).into(), name: wild_name(rng).into() }),
        4 => Command::Drawing(DrawingCmd::SetPaneItemHidden { key: Some(wild_text(rng).into()), hidden: true }),
        5 => Command::Drawing(DrawingCmd::SelectPaneItems { keys: vec![wild_text(rng).into(), wild_text(rng).into()] }),
        6 => Command::Drawing(DrawingCmd::SetShapeStyle { n: w(rng) }),
        7 => Command::Drawing(DrawingCmd::ChangeShape { prst: wild_text(rng).into() }),
        8 => Command::Drawing(DrawingCmd::SetShapeOutline {
            color: None,
            weight_pt: opt(rng),
            dash: Some(wild_text(rng).into()),
            compound: Some(wild_text(rng).into()),
        }),
        9 => Command::Drawing(DrawingCmd::SetTextboxOptions {
            o: TextboxOptions {
                margin_l_pt: opt(rng),
                margin_t_pt: opt(rng),
                margin_r_pt: opt(rng),
                margin_b_pt: opt(rng),
                anchor: Some(wild_text(rng).into()),
                direction: Some(wild_text(rng).into()),
                ..TextboxOptions::default()
            },
        }),
        10 => Command::Drawing(DrawingCmd::SetShapeEffect { kind: EFFECTS[rng.below(6)], preset: Some(wild_text(rng).into()) }),
        11 => Command::Drawing(DrawingCmd::InsertTextbox {
            id: wild_text(rng).into(),
            page: Some(wild_u32(rng)),
            theme_accent1: Some(wild_text(rng).into()),
            lang: wild_text(rng).into(),
            user: None,
        }),
        12 => Command::Drawing(DrawingCmd::InsertTextbox {
            id: "user:x".into(),
            page: None,
            theme_accent1: None,
            lang: "en".into(),
            user: Some(GalleryBox {
                xml: ["", "<w:drawing/>", "<w:drawing><wp:inline><wp:docPr id=\"1\" name=\"x\"/></wp:inline></w:drawing>"][rng.below(3)]
                    .into(),
            }),
        }),
        13 => Command::Drawing(DrawingCmd::LinkTextboxTo { key: wild_text(rng).into() }),
        14 => Command::Drawing(DrawingCmd::CreateDrawnTextbox {
            page: wild_u32(rng),
            rect: ObjRect { x: w(rng), y: w(rng), w: w(rng), h: w(rng) },
            vertical: true,
            anchor_para: Some(wild_u32(rng)),
            lang: "es".into(),
        }),
        15 => Command::Drawing(DrawingCmd::PlaceAt {
            frame: ObjFrame { page_w: w(rng), col_w: w(rng), ..ObjFrame::default() },
            x: w(rng),
            y: w(rng),
        }),
        16 => Command::Drawing(DrawingCmd::AlignObjects { kind: AlignKind::DistributeH, rects: vec![ff(), ff()] }),
        17 => Command::Drawing(DrawingCmd::SetShapePictureFill { data: bytes(rng), ext: wild_text(rng).into(), tile: rng.below(2) == 0 }),
        18 => Command::Picture(PictureCmd::SetSize { width_pt: opt(rng), height_pt: opt(rng), lock_aspect: Some(true) }),
        19 => Command::Picture(PictureCmd::SetRotation { deg: w(rng) }),
        20 => Command::Picture(PictureCmd::Rotate { deg: w(rng) }),
        21 => Command::Picture(PictureCmd::SetCrop { l: w(rng), t: w(rng), r: w(rng), b: w(rng) }),
        22 => Command::Picture(PictureCmd::CropToAspect { w: w(rng), h: w(rng) }),
        23 => Command::Picture(PictureCmd::CropToShape { prst: wild_text(rng).into() }),
        24 => Command::Picture(PictureCmd::CropFillFit { fill: rng.below(2) == 0, natural_pt: Some((w(rng), w(rng))) }),
        25 => Command::Picture(PictureCmd::SetTransparency { pct: w(rng) }),
        26 => Command::Picture(PictureCmd::SetAdjust {
            o: AdjustArg { sharpen: opt(rng), bright: opt(rng), contrast: opt(rng), saturation: opt(rng), temperature: opt(rng) },
        }),
        27 => Command::Picture(PictureCmd::Compress { ppi: w(rng), delete_cropped: true, all: rng.below(2) == 0 }),
        28 => Command::Picture(PictureCmd::Insert { data: bytes(rng), ext: wild_text(rng).into(), ole: None, max_width_twips: opt(rng) }),
        29 => Command::Picture(PictureCmd::Insert {
            data: bytes(rng),
            ext: "png".into(),
            ole: Some(OleArg {
                prog_id: wild_text(rng).into(),
                data: bytes(rng),
                ext: wild_text(rng).into(),
                width_twips: w(rng),
                height_twips: w(rng),
            }),
            max_width_twips: None,
        }),
        30 => Command::Picture(PictureCmd::Change { data: bytes(rng), ext: wild_text(rng).into() }),
        31 => Command::Picture(PictureCmd::SetPosition { preset: wild_text(rng).into(), facts: ff() }),
        32 => Command::Picture(PictureCmd::SetMoveWithText { on: true, facts: ff() }),
        33 => Command::Picture(PictureCmd::SetLayout {
            o: Box::new(LayoutInput {
                width_pt: opt(rng),
                height_pt: opt(rng),
                rotation: opt(rng),
                dist_pt: Some([opt(rng), opt(rng), opt(rng), opt(rng)]),
                pos_h: Some(LayoutPos {
                    relative_from: wild_text(rng).into(),
                    align: Some(wild_text(rng).into()),
                    offset_pt: opt(rng),
                    pct: opt(rng),
                }),
                rel_width: Some(Some((wild_text(rng).into(), w(rng)))),
                ..LayoutInput::default()
            }),
            facts: ff(),
        }),
        34 => Command::Picture(PictureCmd::SetEffect { kind: EFFECTS[rng.below(6)], preset: Some(wild_text(rng).into()) }),
        35 => Command::Picture(PictureCmd::SetRecolor { arg: RecolorArg::Preset(wild_text(rng).into()) }),
        36 => Command::Picture(PictureCmd::SetTransparentColor { hex: Some(wild_text(rng).into()) }),
        37 => Command::Picture(PictureCmd::Align { kind: AlignKind::Center, relative_to: Some(wild_text(rng).into()), rects: vec![ff()] }),
        38 => Command::Picture(PictureCmd::SetBorder { color: None, weight_pt: opt(rng), dash: Some(wild_text(rng).into()) }),
        _ => Command::Picture(PictureCmd::Reset { size: true, natural_pt: Some((w(rng), w(rng))) }),
    };
    Step::Cmd(cmd)
}

/// A blank document with `pics` pictures (each in its own paragraph, `px` x `px` decoded) and
/// `boxes` gallery text boxes, saved.
pub fn doc_with_objects(pics: usize, boxes: usize, px: u32) -> Vec<u8> {
    use uo_docx::session::ops::pictures::PictureCmd as P;
    use uo_docx::session::{FixedEnv, OpenOptions};
    let mut cx = Cx::new();
    let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    let pixels: Arc<[u8]> = Arc::from(pattern(px, px));
    for i in 0..pics {
        let mut data = PNG.to_vec();
        data.extend_from_slice(&u32::try_from(i).unwrap().to_le_bytes());
        s.exec(Command::Text(TextCmd::Type { text: Utf16::from("para ") }), &mut cx.cx()).unwrap();
        let out = s
            .exec(Command::Picture(P::Insert { data: Arc::from(data), ext: "png".into(), ole: None, max_width_twips: None }), &mut cx.cx())
            .unwrap();
        if let Outcome::NeedsJobs(jobs) = out {
            for j in jobs {
                s.complete(j.id(), JobResult::Image { width: px, height: px, rgba: Some(pixels.clone()) }, &mut cx.cx()).unwrap();
            }
        }
        s.exec(Command::Text(TextCmd::Enter), &mut cx.cx()).unwrap();
    }
    for _ in 0..boxes {
        let id = "simple".into();
        s.exec(
            Command::Drawing(DrawingCmd::InsertTextbox { id, page: None, theme_accent1: None, lang: "en".into(), user: None }),
            &mut cx.cx(),
        )
        .unwrap();
        s.exec(Command::Text(TextCmd::Enter), &mut cx.cx()).unwrap();
    }
    s.save_docx().unwrap()
}
