//! Bounded no-panic tests of the editor (frontier B8, security, seconds): random operation scripts over
//! hostile decks (the XML parts of generated decks truncated, cut, duplicated, byte-flipped and spliced with
//! hostile tokens, rebuilt with `Deck::from_parts`) with hostile arguments (NaN and huge frames, markup in
//! texts and ids, unknown layouts). Errors are expected and must leave the package untouched; a panic, a
//! hang or an unbounded allocation is a finding. `UO_EDIT_NOPANIC_ROUNDS=<n>` multiplies the mutants for a
//! long run.
#![cfg(feature = "edit")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::cast_possible_truncation)]

#[path = "support/edit_script.rs"]
mod script;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use script::{Rng, step};
use uo_pptx::edit::{Editor, Op};
use uo_pptx::{Deck, Limits};

const TOKENS: &[&str] = &[
    "<",
    ">",
    "/>",
    "</",
    "<![CDATA[",
    "]]>",
    "<!--",
    "&#xFFFFFFFF;",
    "&#0;",
    "&bogus;",
    "\"",
    "='",
    "id=\"99999999999999999999\"",
    "r:id=\"rId1\"",
    "<p:grpSp>",
    "</p:grpSp>",
    "<p:sp>",
    "</p:sp>",
    "<a:p>",
    "</a:p>",
    "<p:cNvPr id=\"2\"",
    "<p:ph type=\"title\"/>",
    "<mc:AlternateContent>",
    "<p14:creationId val=\"1\"/>",
    "<Relationship Id=\"rId9\" Type=\"x/slide\" Target=\"../slides/slide1.xml\"/>",
];

fn deck_bytes(n: &str) -> Vec<u8> {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/decks/b1a").join(format!("{n}.pptx"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn mutate(rng: &mut Rng, src: &[u8]) -> Vec<u8> {
    let mut b = src.to_vec();
    if b.is_empty() {
        return b;
    }
    match rng.below(6) {
        0 => b.truncate(rng.below(b.len())),
        1 => {
            let a = rng.below(b.len());
            let e = (a + rng.below(200)).min(b.len());
            b.drain(a..e);
        }
        2 => {
            let a = rng.below(b.len());
            let e = (a + rng.below(300)).min(b.len());
            let piece = b[a..e].to_vec();
            let at = rng.below(b.len());
            b.splice(at..at, piece);
        }
        3 => {
            for _ in 0..1 + rng.below(8) {
                let i = rng.below(b.len());
                b[i] = (rng.next() & 0xFF) as u8;
            }
        }
        4 => {
            let t = TOKENS[rng.below(TOKENS.len())].as_bytes().to_vec();
            let at = rng.below(b.len());
            b.splice(at..at, t);
        }
        _ => {
            let i = rng.below(b.len());
            b[i] = b'<';
        }
    }
    b
}

/// A script over an editor: every step may fail; the package of a failed step is what it was.
fn run_script(ed: &mut Editor, rng: &mut Rng, steps: usize) {
    let mut clip = None;
    for _ in 0..steps {
        step(ed, rng, &mut clip);
    }
    while ed.undo().is_some() {}
    while ed.redo().is_some() {}
    let _ = ed.save();
}

#[test]
fn random_scripts_over_mutated_decks_never_panic() {
    let rounds: usize = std::env::var("UO_EDIT_NOPANIC_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
    let limits = Limits { max_group_depth: 32, max_shapes_per_slide: 5_000, max_table_cells: 50_000, ..Limits::default() };
    let t0 = Instant::now();
    let mut rng = Rng(0xA5A5_1234_9E37_79B9);
    let mut scripts = 0usize;
    for n in ["0000", "0001", "0003", "0006", "0008", "0020"] {
        let base = Deck::open(&deck_bytes(n), limits).unwrap();
        let parts: Vec<(Box<str>, Arc<[u8]>)> = base.parts().entries().iter().map(|e| (e.name.clone(), Arc::clone(&e.bytes))).collect();
        for (pi, (name, bytes)) in parts.iter().enumerate() {
            if !(name.ends_with(".xml") || name.ends_with(".rels")) {
                continue;
            }
            for _ in 0..rounds {
                let mut all = parts.clone();
                if let Some(slot) = all.get_mut(pi) {
                    slot.1 = Arc::from(mutate(&mut rng, bytes));
                }
                if let Ok(deck) = Deck::from_parts(all, limits) {
                    let mut ed = Editor::new(deck);
                    run_script(&mut ed, &mut rng, 14);
                    scripts += 1;
                }
            }
        }
    }
    eprintln!("edit nopanic: {scripts} scripts in {:?}", t0.elapsed());
    assert!(scripts > 60);
}

#[test]
fn a_failed_operation_leaves_the_package_as_it_was() {
    let ed = Editor::open(&deck_bytes("0008"), Limits::default()).unwrap();
    let before: Vec<(Box<str>, Arc<[u8]>)> = ed.deck().parts().entries().iter().map(|e| (e.name.clone(), Arc::clone(&e.bytes))).collect();
    let n = before.len();
    // a deck whose presentation part lost its list: duplicating a slide fails after the copy was made
    let mut hostile = before.clone();
    for (name, bytes) in &mut hostile {
        if &**name == "ppt/presentation.xml" {
            let t = String::from_utf8_lossy(bytes).replace("<p:sldIdLst>", "<p:sldIdLstX>").replace("</p:sldIdLst>", "</p:sldIdLstX>");
            *bytes = Arc::from(t.into_bytes());
        }
    }
    let mut hed = Editor::new(Deck::from_parts(hostile.clone(), Limits::default()).unwrap());
    let r = hed.apply(Op::DuplicateSlide { i: 0 });
    assert!(r.is_err() || hed.deck().parts().len() == n, "{r:?}");
    for (name, bytes) in &hostile {
        assert_eq!(hed.deck().parts().bytes(name).map(|b| &b[..]), Some(&bytes[..]), "{name}");
    }
    assert!(!hed.can_undo());
}

#[test]
fn hostile_arguments_on_a_clean_deck_do_not_panic() {
    let mut rng = Rng(0xBEEF_F00D_1234_5678);
    for n in ["0000", "0003", "0008", "0010"] {
        for k in 0..8 {
            let mut ed = Editor::open(&deck_bytes(n), Limits::default()).unwrap();
            let mut r = Rng(rng.next() ^ k);
            run_script(&mut ed, &mut r, 60);
        }
    }
}
