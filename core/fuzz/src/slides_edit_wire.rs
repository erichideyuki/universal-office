//! `slides_edit_wire`: the editor messages of `<universal-slides engine="rust">` (schema `core/schema/slides_edit.uos`, the
//! generated codec and bind of uo-wasm `src/slides_edit/`, compiled in through `crate::slides_edit::{codec, wire}`) decoded,
//! bound to `uo_pptx::edit` and run on an editor over the blank presentation — the path of `uo_slides_editor_apply` and its
//! siblings without the C ABI (uo-wasm cannot be linked next to uo-ffi; the ABI itself is covered by
//! `uo-wasm/tests/slides_edit_native.rs`).
//!
//! Input: byte 0 selects the message — 0 `Apply`, 1 `Query`, 2 `FontFaces`, 3 `Blank`, 4 `Apply` then undo and redo — and the
//! rest is the message, hostile bytes included.
//!
//! Invariants beyond "no panic, no hang": (1) a message the decoder accepts re-encodes to bytes that decode and re-encode to the
//! SAME bytes (the canonical form is stable); (2) a command that failed leaves the package exactly as it was (no changed part);
//! (3) the `EditResult` of every command encodes and decodes; (4) a command that changed the package, undone and redone, gives
//! the same parts back; (5) a blank presentation from any accepted options saves and reopens.
use std::sync::Arc;

use uo_pptx::edit::blank::{BlankDeckOptions, blank_deck_store};
use uo_pptx::edit::doc::{DocProperties, ShowSettingsSpec};
use uo_pptx::edit::pkg::TransitionSpec;
use uo_pptx::edit::shapes::{ArrangeMode, BulletKind, Frame, RunFormat};
use uo_pptx::edit::{Editor, Op, Target, changes_since};
use uo_pptx::{Deck, Limits};

use crate::slides_edit::codec as w;
use crate::slides_edit::wire::{FromWire, IntoWire};

fn limits() -> Limits {
    Limits { max_slides: 64, max_history_bytes: 8 << 20, ..Limits::default() }
}

fn stable<M>(
    data: &[u8],
    dec: fn(&[u8], &mut w::Tables, &uo_codec::Limits) -> Result<M, uo_codec::DecodeError>,
    enc: fn(&mut Vec<u8>, &w::Delta, &M) -> Result<(), uo_codec::EncodeError>,
) -> Option<M> {
    let m = dec(data, &mut w::Tables::new(), &w::limits()).ok()?;
    let mut a = Vec::new();
    enc(&mut a, &w::Delta::default(), &m).expect("a decoded message encodes");
    let m2 = dec(&a, &mut w::Tables::new(), &w::limits()).expect("a re-encoded message decodes");
    let mut b = Vec::new();
    enc(&mut b, &w::Delta::default(), &m2).expect("encodes");
    assert_eq!(a, b, "the canonical form is stable");
    Some(m)
}

fn digest(d: &Deck) -> Vec<(Box<str>, Arc<[u8]>)> {
    d.parts().entries().iter().map(|e| (e.name.clone(), Arc::clone(&e.bytes))).collect()
}

pub fn slides_edit_wire(data: &[u8]) {
    let Some((&sel, msg)) = data.split_first() else { return };
    let mut ed = Editor::blank(&BlankDeckOptions::default(), limits());
    ed.set_random_seed(1);
    match sel % 5 {
        0 | 4 => {
            let Some(m) = stable(msg, w::decode_apply, w::encode_apply) else { return };
            let Ok(op) = Op::from_wire(m.op) else { return };
            let before = ed.deck().parts().snapshot();
            let r = ed.apply(op);
            let (changed, gone) = changes_since(ed.deck().parts(), &before);
            let n_changes = changed.len() + gone.len();
            let outcome = match r {
                Ok(o) => w::ApplyOutcome::Ok(o.into_wire().expect("an outcome has a wire form")),
                Err(e) => {
                    assert_eq!(n_changes, 0, "a failed command changed the package");
                    w::ApplyOutcome::Err(e.to_string().encode_utf16().collect())
                }
            };
            let changes =
                changed.iter().map(|e| w::PartChange { name: e.name.encode_utf16().collect(), bytes: Some(e.bytes.to_vec()) }).collect();
            let state = w::UiState {
                index: ed.index(),
                selected: ed.selected().map(|s| s.encode_utf16().collect()),
                can_undo: ed.can_undo(),
                can_redo: ed.can_redo(),
                undo_label: None,
                redo_label: None,
                dirty: ed.is_dirty(),
                slides: u32::try_from(ed.slide_count()).unwrap_or(u32::MAX),
                embed: false,
                subset: false,
            };
            let mut out = Vec::new();
            w::encode_edit_result(&mut out, &w::Delta::default(), &w::EditResult { outcome, state, changes }).expect("encodes");
            w::decode_edit_result(&out, &mut w::Tables::new(), &w::limits()).expect("an EditResult decodes");
            if sel % 5 == 4 && n_changes > 0 {
                let after = digest(ed.deck());
                ed.undo();
                ed.redo();
                let again = digest(ed.deck());
                assert_eq!(after.len(), again.len());
                for ((a, ab), (b, bb)) in after.iter().zip(&again) {
                    assert_eq!(a, b, "redo restores the part order");
                    assert_eq!(ab, bb, "redo restores the bytes");
                }
            }
        }
        1 => {
            let Some(m) = stable(msg, w::decode_query, w::encode_query) else { return };
            match m.q {
                w::QueryOp::FindText(f) => {
                    let hits = ed.find_text(&String::from_utf16_lossy(&f.query), f.match_case);
                    let wire: Vec<w::TupI32Utf16> = hits.into_wire().expect("hits have a wire form");
                    drop(wire);
                }
                w::QueryOp::DefaultNewLayout => drop(ed.default_new_layout()),
                w::QueryOp::FontEmbedding => drop(ed.font_embedding()),
            }
        }
        2 => {
            let Some(m) = stable(msg, w::decode_font_faces, w::encode_font_faces) else { return };
            let Ok(faces) = <Vec<Option<Arc<[u8]>>>>::from_wire(m.faces) else { return };
            ed.set_font_embedding(Some(true), Some(true));
            let reqs = ed.font_requests();
            let lookup = |r: &uo_fonts::FaceRequest| reqs.iter().position(|q| q == r).and_then(|i| faces.get(i).cloned().flatten());
            if let Ok(saved) = ed.save_with_fonts(&lookup) {
                Deck::open(&saved.bytes, limits()).expect("the saved package reopens");
            }
        }
        _ => {
            let Some(m) = stable(msg, w::decode_blank, w::encode_blank) else { return };
            let Ok(opts) = BlankDeckOptions::from_wire(m.opts) else { return };
            let bytes = blank_deck_store(&opts).save().expect("a blank presentation saves");
            Deck::open(&bytes, limits()).expect("a blank presentation reopens");
        }
    }
}

fn apply(op: Op) -> Vec<u8> {
    let mut out = Vec::new();
    w::encode_apply(&mut out, &w::Delta::default(), &w::Apply { op: op.into_wire().expect("a wire op") }).expect("encodes");
    out
}

/// One message of every command, the queries, font answers and blank options, each with its selector byte.
pub fn slides_edit_wire_seeds() -> Vec<Vec<u8>> {
    let f = Some(Frame { x: 12700.0, y: 25400.0, cx: 914400.0, cy: 457200.0 });
    let ops = vec![
        Op::AddSlide { layout: None },
        Op::DuplicateSlide { i: 0 },
        Op::DeleteSlide { i: 0 },
        Op::MoveSlide { from: 0, to: 1 },
        Op::SetSlideHidden { i: 0, hidden: true },
        Op::SetSlideLayout { i: 0, layout: "ppt/slideLayouts/slideLayout2.xml".into() },
        Op::SetSlideBackground { target: Target::All, color: Some("#FF0000".into()) },
        Op::SetSlideTransition {
            target: Target::Slide(0),
            spec: Some(TransitionSpec {
                type_: "push".into(),
                dir: Some("l".into()),
                speed: None,
                advance_on_click: Some(false),
                advance_after_ms: Some(2500.0),
            }),
        },
        Op::SetShowSettings(ShowSettingsSpec { loop_: Some(true), use_timings: None, show_animation: Some(false), kiosk: None }),
        Op::InsertTextBox { text: "Olá <b> & \u{1F600}".into(), frame: f },
        Op::InsertShape { prst: "ellipse".into(), frame: None },
        Op::InsertImage { bytes: Arc::from(vec![0x89, b'P', b'N', b'G']), mime: "image/png".into(), frame: f, size_px: Some((2.0, 2.0)) },
        Op::SetShapeText { shape: "2".into(), text: "a\nb".into() },
        Op::Typing { slide: 0, shape: "2".into(), text: "typed".into() },
        Op::FormatText {
            shape: "2".into(),
            f: RunFormat { bold: Some(true), size: Some(28.0), font: Some("Carlito".into()), ..RunFormat::default() },
        },
        Op::GrowFont { shape: "2".into(), dir: 1, base: 18.0 },
        Op::AlignText { shape: "2".into(), algn: "ctr".into() },
        Op::SetBullets { shape: "2".into(), kind: BulletKind::Number },
        Op::ClearFormatting { shape: "2".into() },
        Op::SetLink { shape: "2".into(), url: Some("https://example.com/".into()) },
        Op::SetShapeFrame { shape: "2".into(), frame: Frame { x: f64::NAN, y: 1e18, cx: -1.0, cy: 0.0 } },
        Op::SetShapeFill { shape: "2".into(), color: None },
        Op::SetShapeOutline { shape: "2".into(), color: Some("zz".into()) },
        Op::ArrangeShape { shape: "2".into(), mode: ArrangeMode::Back },
        Op::DeleteShape { shape: "2".into() },
        Op::SetSlideNotes { i: 0, text: "notes".into() },
        Op::SetProperties(DocProperties { title: Some("T & <1>".into()), ..DocProperties::default() }),
        Op::ReplaceText { query: "a".into(), replacement: "$&".into(), match_case: false, hit: Some((0, "2".into())) },
    ];
    let mut out = Vec::new();
    for op in ops {
        let m = apply(op);
        out.push([&[0u8][..], &m].concat());
        out.push([&[4u8][..], &m].concat());
    }
    let q = |q: w::QueryOp| {
        let mut v = Vec::new();
        w::encode_query(&mut v, &w::Delta::default(), &w::Query { q }).expect("encodes");
        [&[1u8][..], &v].concat()
    };
    out.push(q(w::QueryOp::FindText(w::QueryFind { query: "a".encode_utf16().collect(), match_case: true })));
    out.push(q(w::QueryOp::DefaultNewLayout));
    out.push(q(w::QueryOp::FontEmbedding));
    let mut faces = Vec::new();
    let msg = w::FontFaces { faces: vec![w::OptBytes { value: Some(vec![0, 1, 0, 0]) }, w::OptBytes { value: None }] };
    w::encode_font_faces(&mut faces, &w::Delta::default(), &msg).expect("encodes");
    out.push([&[2u8][..], &faces].concat());
    let mut blank = Vec::new();
    let opts = BlankDeckOptions { wide: false, title: Some("x".into()), lang: "pt-BR".into(), now: "2026-01-02T03:04:00Z".into() };
    w::encode_blank(&mut blank, &w::Delta::default(), &w::Blank { opts: opts.into_wire().expect("wire") }).expect("encodes");
    out.push([&[3u8][..], &blank].concat());
    out.push(vec![0, 0x55, 0x4f, 0x53, 0x45, 0, 0, 0]);
    out
}
