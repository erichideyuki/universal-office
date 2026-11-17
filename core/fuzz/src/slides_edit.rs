//! `slides_edit`: the editing session of `uo-pptx` (frontier B8) under random scripts over an arbitrary deck.
//!
//! Input: byte 0 selects the script length (1..=48 steps) and whether the base is the blank deck (bit 7 clear) or
//! the `.pptx` that follows the 8-byte script seed (bit 7 set, opened with small limits; a deck that does not open falls
//! back to the blank one). The seed drives a deterministic generator of operations (hostile arguments mixed in: NaN and
//! huge frames, markup in texts and ids, unknown layouts), undo, redo, selection and copy/paste.
//!
//! Invariants beyond "no panic, no hang": (1) an operation that returns `Err` leaves the package exactly as it was;
//! (2) an operation that changed the package is undone and redone to the same bytes (each part, in order); (3) the saved
//! package reopens.
use std::sync::Arc;

use uo_pptx::edit::blank::BlankDeckOptions;
use uo_pptx::edit::doc::{DocProperties, ShapeClip, ShowSettingsSpec};
use uo_pptx::edit::pkg::TransitionSpec;
use uo_pptx::edit::shapes::{ArrangeMode, BulletKind, Frame, RunFormat};
use uo_pptx::edit::{Editor, Op, Target};
use uo_pptx::{Deck, Limits};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { usize::try_from(self.next() % n as u64).unwrap_or(0) }
    }
}

const TEXTS: &[&str] = &["Hello", "Olá ção", "a\nb\n\nc", "A&B <C> \"D\"", "😀𝒜", "", "$&$1$'", "</a:t><a:t>", "<p:sp>", "x"];
const COLORS: &[&str] = &["FF0000", "#00aa00", "", "zz", "1234567890", "#"];
const NUMS: &[f64] = &[0.0, -1.0, 1.5, 12700.0, 1e9, -1e15, 4.5e18, f64::NAN, f64::INFINITY, 99999.9];

fn limits() -> Limits {
    Limits {
        max_slides: 64,
        max_shapes_per_slide: 2_000,
        max_group_depth: 24,
        max_table_cells: 10_000,
        max_xml_cache_bytes: 8 << 20,
        max_cached_slides: 4,
        max_history_bytes: 8 << 20,
        ..Limits::default()
    }
}

fn frame(r: &mut Rng) -> Frame {
    Frame { x: NUMS[r.below(NUMS.len())], y: NUMS[r.below(NUMS.len())], cx: NUMS[r.below(NUMS.len())], cy: NUMS[r.below(NUMS.len())] }
}

fn color(r: &mut Rng) -> Option<String> {
    if r.below(4) == 0 { None } else { Some(COLORS[r.below(COLORS.len())].to_owned()) }
}

fn random_op(ed: &mut Editor, r: &mut Rng) -> Op {
    let n = ed.slide_count();
    let cur = i64::from(ed.index());
    let slide = |r: &mut Rng| -> i64 { i64::try_from(r.below(n + 2)).unwrap_or(0) - 1 };
    let shapes: Vec<String> = ed.slide_model(cur).map(|m| m.shapes.iter().filter(|s| s.inherited != Some(true)).map(|s| s.id.to_string()).collect()).unwrap_or_default();
    let shape = |r: &mut Rng| -> String { if shapes.is_empty() || r.below(10) == 0 { format!("{}", r.below(20)) } else { shapes[r.below(shapes.len())].clone() } };
    let text = |r: &mut Rng| TEXTS[r.below(TEXTS.len())].to_owned();
    match r.below(28) {
        0 => Op::AddSlide { layout: None },
        1 => Op::DuplicateSlide { i: slide(r) },
        2 => Op::DeleteSlide { i: slide(r) },
        3 => Op::MoveSlide { from: slide(r), to: slide(r) },
        4 => Op::SetSlideHidden { i: slide(r), hidden: r.below(2) == 0 },
        5 => Op::SetSlideBackground { target: if r.below(4) == 0 { Target::All } else { Target::Slide(slide(r)) }, color: color(r) },
        6 => Op::SetSlideTransition {
            target: Target::Slide(slide(r)),
            spec: if r.below(5) == 0 { None } else { Some(TransitionSpec { type_: ["fade", "push", "split", "none", "wheel"][r.below(5)].to_owned(), dir: Some("l".into()), speed: Some("slow".into()), advance_on_click: Some(r.below(2) == 0), advance_after_ms: Some(NUMS[r.below(NUMS.len())]) }) },
        },
        7 => Op::SetShowSettings(ShowSettingsSpec { loop_: Some(r.below(2) == 0), use_timings: None, show_animation: Some(false), kiosk: Some(r.below(2) == 0) }),
        8 => Op::SetShapeText { shape: shape(r), text: text(r) },
        9 => Op::Typing { slide: cur, shape: shape(r), text: text(r) },
        10 => Op::FormatText { shape: shape(r), f: RunFormat { bold: Some(true), size: Some(NUMS[r.below(NUMS.len())]), font: Some(text(r)), color: color(r), ..RunFormat::default() } },
        11 => Op::GrowFont { shape: shape(r), dir: if r.below(2) == 0 { 1 } else { -1 }, base: NUMS[r.below(NUMS.len())] },
        12 => Op::AlignText { shape: shape(r), algn: ["l", "ctr", "\"", ""][r.below(4)].to_owned() },
        13 => Op::SetBullets { shape: shape(r), kind: [BulletKind::Bullet, BulletKind::Number, BulletKind::None][r.below(3)] },
        14 => Op::ClearFormatting { shape: shape(r) },
        15 => Op::SetLink { shape: shape(r), url: if r.below(3) == 0 { None } else { Some(text(r)) } },
        16 => Op::InsertTextBox { text: text(r), frame: if r.below(2) == 0 { None } else { Some(frame(r)) } },
        17 => Op::InsertShape { prst: ["rect", "line", "\"><x", ""][r.below(4)].to_owned(), frame: if r.below(2) == 0 { None } else { Some(frame(r)) } },
        18 => Op::InsertImage { bytes: Arc::from(vec![0x89u8, 0x50, u8::try_from(r.below(256)).unwrap_or(0)]), mime: ["image/png", "image/svg+xml", ""][r.below(3)].to_owned(), frame: None, size_px: Some((NUMS[r.below(NUMS.len())], NUMS[r.below(NUMS.len())])) },
        19 => Op::SetShapeFrame { shape: shape(r), frame: frame(r) },
        20 => Op::SetShapeFill { shape: shape(r), color: color(r) },
        21 => Op::SetShapeOutline { shape: shape(r), color: color(r) },
        22 => Op::ArrangeShape { shape: shape(r), mode: [ArrangeMode::Front, ArrangeMode::Back, ArrangeMode::Forward, ArrangeMode::Backward][r.below(4)] },
        23 => Op::DeleteShape { shape: shape(r) },
        24 => Op::SetSlideNotes { i: slide(r), text: text(r) },
        25 => Op::SetProperties(DocProperties { title: Some(text(r)), author: Some(text(r)), ..DocProperties::default() }),
        26 => Op::ReplaceText { query: ["e", "<", "&", ""][r.below(4)].to_owned(), replacement: text(r), match_case: r.below(2) == 0, hit: None },
        _ => Op::SetSlideLayout { i: slide(r), layout: "ppt/slideLayouts/slideLayout1.xml".into() },
    }
}

/// Every part in order, to compare packages exactly.
fn parts(ed: &Editor) -> Vec<(Box<str>, Arc<[u8]>)> {
    ed.deck().parts().entries().iter().map(|e| (e.name.clone(), Arc::clone(&e.bytes))).collect()
}

fn same(a: &[(Box<str>, Arc<[u8]>)], b: &[(Box<str>, Arc<[u8]>)]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.0 == y.0 && x.1[..] == y.1[..])
}

/// The target.
pub fn slides_edit(data: &[u8]) {
    let Some((&sel, rest)) = data.split_first() else { return };
    let steps = 1 + usize::from(sel & 0x3f) % 48;
    let (seed_bytes, deck_bytes) = rest.split_at(rest.len().min(8));
    let mut seed = [0u8; 8];
    seed[..seed_bytes.len()].copy_from_slice(seed_bytes);
    let mut r = Rng(u64::from_le_bytes(seed) | 1);
    let mut ed = if sel & 0x80 != 0 {
        let opc = uo_opc::Limits { max_xml_part_bytes: 4 << 20, max_binary_part_bytes: 8 << 20, max_total_bytes: 32 << 20, max_entries: 512, max_xml_depth: 256, max_xml_elements: 400_000 };
        match Deck::open_with(deck_bytes, limits(), &opc) {
            Ok(d) => Editor::new(d),
            Err(_) => Editor::blank(&BlankDeckOptions::default(), limits()),
        }
    } else {
        Editor::blank(&BlankDeckOptions::default(), limits())
    };
    let mut clip: Option<ShapeClip> = None;
    for _ in 0..steps {
        match r.below(14) {
            0 => {
                ed.undo();
            }
            1 => {
                ed.redo();
            }
            2 => {
                let id = ed.slide_model(i64::from(ed.index())).and_then(|m| m.shapes.iter().find(|s| s.inherited != Some(true)).map(|s| s.id.to_string()));
                ed.select(id.as_deref());
            }
            3 => {
                if let Some(id) = ed.selected().map(str::to_owned) {
                    clip = ed.copy_shape(&id).unwrap_or(None);
                }
            }
            4 => {
                if let Some(c) = clip.clone() {
                    let _ = ed.apply(Op::PasteShape { clip: c });
                }
            }
            _ => {
                let op = random_op(&mut ed, &mut r);
                let label = format!("{op:?}");
                // an insert or paste on a deck without slides adds one first (its own step): not a partial change
                let nested = ed.slide_count() == 0;
                let before = parts(&ed);
                let can_undo = ed.can_undo();
                match ed.apply(op) {
                    Err(_) => assert!(nested || same(&before, &parts(&ed)), "an operation that failed changed the package: {label}"),
                    Ok(_) => {
                        // a nested step (an insert on a deck without slides adds one first) may add two steps
                        if !same(&before, &parts(&ed)) && ed.can_undo() && (can_undo || ed.undo_label().is_some()) {
                            let after = parts(&ed);
                            if ed.undo().is_some() && ed.redo().is_some() {
                                assert!(same(&after, &parts(&ed)), "undo then redo did not restore the package");
                            }
                        }
                    }
                }
            }
        }
    }
    if let Ok(saved) = ed.save() {
        assert!(Deck::open(&saved, limits()).is_ok(), "a saved package must reopen");
    }
}

/// Seeds: the blank deck with a few scripts, and a generated deck.
pub fn slides_edit_seeds(repo: &std::path::Path) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for s in 0..6u8 {
        let mut v = vec![0x10 + s * 4];
        v.extend_from_slice(&u64::from(s + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15).to_le_bytes());
        out.push(v);
    }
    let dir = repo.join("core/crates/uo-pptx/tests/captures/decks/b1a");
    for n in ["0003", "0008"] {
        if let Ok(b) = std::fs::read(dir.join(format!("{n}.pptx"))) {
            let mut v = vec![0x80 | 0x20];
            v.extend_from_slice(&0xC0FF_EE12_3456_789Au64.to_le_bytes());
            v.extend_from_slice(&b);
            out.push(v);
        }
    }
    out
}
