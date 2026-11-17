//! Random editing scripts for the memory and no-panic tests of the editor (frontier B8): `random_op`
//! picks an operation from what the current slide has (shape ids from the model, slide indices), with
//! hostile arguments mixed in (huge numbers, lone markup, empty strings), and `step` applies it or an
//! undo/redo.
#![allow(dead_code, clippy::unwrap_used, clippy::cast_possible_truncation, clippy::cast_possible_wrap)]

use std::sync::Arc;

use uo_pptx::edit::doc::{DocProperties, ShowSettingsSpec};
use uo_pptx::edit::pkg::TransitionSpec;
use uo_pptx::edit::shapes::{ArrangeMode, BulletKind, Frame, RunFormat};
use uo_pptx::edit::{Editor, Op, Target};

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    pub fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }
}

const TEXTS: &[&str] = &["Hello", "Olá ção", "a\nb\n\nc", "A&B <C> \"D\"", "😀𝒜", "", "$&$1$'", "</a:t><a:t>", "<p:sp>", "x"];
const COLORS: &[&str] = &["FF0000", "#00aa00", "", "zz", "1234567890", "#"];
const PRSTS: &[&str] = &["rect", "ellipse", "line", "\"><x", "star5", ""];
const MIMES: &[&str] = &["image/png", "image/jpeg", "image/svg+xml", "text/plain", ""];
const NUMS: &[f64] = &[0.0, -1.0, 1.5, 12700.0, 1e9, -1e15, 4.5e18, f64::NAN, f64::INFINITY, 99999.9];

fn frame(r: &mut Rng) -> Frame {
    Frame { x: *r.pick(NUMS), y: *r.pick(NUMS), cx: *r.pick(NUMS), cy: *r.pick(NUMS) }
}

fn opt_color(r: &mut Rng) -> Option<String> {
    if r.below(4) == 0 { None } else { Some((*r.pick(COLORS)).to_owned()) }
}

pub fn random_op(ed: &mut Editor, r: &mut Rng) -> Op {
    let n = ed.slide_count();
    let cur = i64::from(ed.index());
    let any_slide = |r: &mut Rng| -> i64 { if r.below(8) == 0 { r.below(n + 3) as i64 - 1 } else { r.below(n.max(1)) as i64 } };
    let shapes: Vec<String> = ed
        .slide_model(cur)
        .map(|m| m.shapes.iter().filter(|s| s.inherited != Some(true)).map(|s| s.id.to_string()).collect())
        .unwrap_or_default();
    let shape = |r: &mut Rng| -> String {
        if shapes.is_empty() || r.below(10) == 0 { format!("{}", r.below(20)) } else { shapes[r.below(shapes.len())].clone() }
    };
    let text = |r: &mut Rng| (*r.pick(TEXTS)).to_owned();
    match r.below(30) {
        0 => Op::AddSlide { layout: None },
        1 => Op::DuplicateSlide { i: any_slide(r) },
        2 => Op::DeleteSlide { i: any_slide(r) },
        3 => Op::MoveSlide { from: any_slide(r), to: any_slide(r) },
        4 => Op::SetSlideHidden { i: any_slide(r), hidden: r.below(2) == 0 },
        5 => {
            Op::SetSlideBackground { target: if r.below(4) == 0 { Target::All } else { Target::Slide(any_slide(r)) }, color: opt_color(r) }
        }
        6 => Op::SetSlideTransition {
            target: Target::Slide(any_slide(r)),
            spec: if r.below(5) == 0 {
                None
            } else {
                Some(TransitionSpec {
                    type_: ["fade", "push", "split", "none", "wheel", "\"x"][r.below(6)].to_owned(),
                    dir: Some("l".into()),
                    speed: Some("slow".into()),
                    advance_on_click: Some(r.below(2) == 0),
                    advance_after_ms: Some(*r.pick(NUMS)),
                })
            },
        },
        7 => Op::SetShowSettings(ShowSettingsSpec {
            loop_: Some(r.below(2) == 0),
            use_timings: None,
            show_animation: Some(false),
            kiosk: Some(r.below(2) == 0),
        }),
        8 => Op::SetShapeText { shape: shape(r), text: text(r) },
        9 => Op::Typing { slide: cur, shape: shape(r), text: text(r) },
        10 => Op::FormatText {
            shape: shape(r),
            f: RunFormat { bold: Some(true), size: Some(*r.pick(NUMS)), font: Some(text(r)), color: opt_color(r), ..RunFormat::default() },
        },
        11 => Op::GrowFont { shape: shape(r), dir: if r.below(2) == 0 { 1 } else { -1 }, base: *r.pick(NUMS) },
        12 => Op::AlignText { shape: shape(r), algn: ["l", "ctr", "\"", ""][r.below(4)].to_owned() },
        13 => Op::SetBullets { shape: shape(r), kind: [BulletKind::Bullet, BulletKind::Number, BulletKind::None][r.below(3)] },
        14 => Op::ClearFormatting { shape: shape(r) },
        15 => Op::SetLink { shape: shape(r), url: if r.below(3) == 0 { None } else { Some(text(r)) } },
        16 => Op::InsertTextBox { text: text(r), frame: if r.below(2) == 0 { None } else { Some(frame(r)) } },
        17 => Op::InsertShape { prst: (*r.pick(PRSTS)).to_owned(), frame: if r.below(2) == 0 { None } else { Some(frame(r)) } },
        18 => Op::InsertImage {
            bytes: Arc::from(vec![0x89u8, 0x50, 0x4e, 0x47, r.below(256) as u8]),
            mime: (*r.pick(MIMES)).to_owned(),
            frame: if r.below(2) == 0 { None } else { Some(frame(r)) },
            size_px: Some((*r.pick(NUMS), *r.pick(NUMS))),
        },
        19 => Op::SetShapeFrame { shape: shape(r), frame: frame(r) },
        20 => Op::SetShapeFill { shape: shape(r), color: opt_color(r) },
        21 => Op::SetShapeOutline { shape: shape(r), color: opt_color(r) },
        22 => Op::ArrangeShape {
            shape: shape(r),
            mode: [ArrangeMode::Front, ArrangeMode::Back, ArrangeMode::Forward, ArrangeMode::Backward][r.below(4)],
        },
        23 => Op::DeleteShape { shape: shape(r) },
        24 => Op::SetSlideNotes { i: any_slide(r), text: text(r) },
        25 => Op::SetProperties(DocProperties { title: Some(text(r)), author: Some(text(r)), ..DocProperties::default() }),
        26 => Op::ReplaceText {
            query: ["e", "a", "<", "&", "ção", ""][r.below(6)].to_owned(),
            replacement: text(r),
            match_case: r.below(2) == 0,
            hit: None,
        },
        27 => Op::SetSlideLayout { i: any_slide(r), layout: "ppt/slideLayouts/slideLayout1.xml".into() },
        _ => Op::SetShapeText { shape: shape(r), text: text(r) },
    }
}

/// One step of a script: an operation (mostly), an undo/redo, a selection or a copy/paste.
pub fn step(ed: &mut Editor, r: &mut Rng, clip: &mut Option<uo_pptx::edit::doc::ShapeClip>) {
    match r.below(16) {
        0 | 1 => {
            ed.undo();
        }
        2 => {
            ed.redo();
        }
        3 => {
            let shape = ed
                .slide_model(i64::from(ed.index()))
                .and_then(|m| m.shapes.iter().find(|s| s.inherited != Some(true)).map(|s| s.id.to_string()));
            ed.select(shape.as_deref());
        }
        4 => {
            let id = ed.selected().map(str::to_owned);
            if let Some(id) = id {
                *clip = ed.copy_shape(&id).unwrap_or(None);
            }
        }
        5 => {
            if let Some(c) = clip.clone() {
                let _ = ed.apply(Op::PasteShape { clip: c });
            }
        }
        6 => {
            let n = ed.slide_count();
            if n > 0 {
                ed.set_index(r.below(n) as u32);
            }
        }
        _ => {
            let op = random_op(ed, r);
            let _ = ed.apply(op);
        }
    }
}
