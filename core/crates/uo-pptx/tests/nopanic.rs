//! Bounded no-panic tests over mutated decks (security, seconds): the XML parts of generated decks are
//! truncated, cut, duplicated, byte-flipped and spliced with hostile tokens, the packages are rebuilt
//! with `Deck::from_parts` and read end to end. Errors are expected; a panic, a hang or an unbounded
//! allocation is a finding. `UO_PPTX_NOPANIC_ROUNDS=<n>` multiplies the mutants per part for a long run.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

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
    "val=\"-1\"",
    "val=\"NaN\"",
    "val=\"99999999999999999999\"",
    "w=\"-9\"",
    "gridSpan=\"99999999\"",
    "rowSpan=\"4294967295\"",
    "<a:ext cx=\"1e999\"/>",
    "r:embed=\"rId1\"",
    "<p:grpSp>",
    "</p:grpSp>",
    "<a:tbl>",
    "<a:tr><a:tc/></a:tr>",
    "<p:ph type=\"title\" idx=\"1\"/>",
    "typeface=\"+mn-lt\"",
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
    match rng.below(7) {
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
                b[i] = u8::try_from(rng.next() & 0xFF).unwrap_or(0);
            }
        }
        4 => {
            let t = TOKENS[rng.below(TOKENS.len())].as_bytes().to_vec();
            let at = rng.below(b.len());
            b.splice(at..at, t);
        }
        5 => {
            // a deeply nested group at a random place
            let depth = 50 + rng.below(400);
            let mut s = String::new();
            for _ in 0..depth {
                s.push_str("<p:grpSp><p:grpSpPr/>");
            }
            let at = rng.below(b.len());
            b.splice(at..at, s.into_bytes());
        }
        _ => {
            let i = rng.below(b.len());
            b[i] = b'<';
        }
    }
    b
}

fn read_all(deck: &mut Deck) {
    let Ok(pres) = deck.presentation().cloned() else { return };
    for i in 0..pres.slides.len().min(6) {
        let i = u32::try_from(i).unwrap();
        let _ = deck.slide(i);
        let _ = deck.slide_notes(i);
    }
    let _ = deck.embedded_fonts();
    let _ = deck.deck_typefaces();
    let _ = deck.save();
}

#[test]
fn mutated_xml_parts_never_panic() {
    let rounds: usize = std::env::var("UO_PPTX_NOPANIC_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
    let limits = Limits { max_group_depth: 32, max_shapes_per_slide: 5_000, max_table_cells: 50_000, ..Limits::default() };
    let t0 = Instant::now();
    let mut mutants = 0usize;
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for n in ["0000", "0001", "0003", "0004", "0006", "0008", "0010", "0020"] {
        let base = Deck::open(&deck_bytes(n), limits).unwrap();
        let parts: Vec<(Box<str>, Arc<[u8]>)> = base.parts().entries().iter().map(|e| (e.name.clone(), Arc::clone(&e.bytes))).collect();
        for (pi, (name, bytes)) in parts.iter().enumerate() {
            if !(name.ends_with(".xml") || name.ends_with(".rels")) {
                continue;
            }
            for _ in 0..rounds {
                let m = mutate(&mut rng, bytes);
                let mut all = parts.clone();
                if let Some(slot) = all.get_mut(pi) {
                    slot.1 = Arc::from(m);
                }
                if let Ok(mut deck) = Deck::from_parts(all, limits) {
                    read_all(&mut deck);
                    mutants += 1;
                }
            }
        }
    }
    eprintln!("nopanic: {mutants} mutants in {:?}", t0.elapsed());
    assert!(mutants > 100);
}

#[test]
fn mutated_packages_never_panic() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let limits = Limits::default();
    let mut ok = 0;
    for n in ["0003", "0008"] {
        let bytes = deck_bytes(n);
        for _ in 0..60 {
            let m = mutate(&mut rng, &bytes);
            if let Ok(mut deck) = Deck::open(&m, limits) {
                read_all(&mut deck);
                ok += 1;
            }
        }
    }
    eprintln!("nopanic packages: {ok} opened");
}

#[test]
fn hostile_shapes_hit_the_limits_not_the_stack() {
    // a group nested 10,000 deep: a typed error at the depth limit (the TS raises a RangeError)
    let mut body = String::new();
    for _ in 0..10_000 {
        body.push_str("<p:grpSp><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"g\"/></p:nvGrpSpPr><p:grpSpPr/>");
    }
    for _ in 0..10_000 {
        body.push_str("</p:grpSp>");
    }
    let base = Deck::open(&deck_bytes("0003"), Limits::default()).unwrap();
    let slide = base.parts().names().find(|n| n.starts_with("ppt/slides/slide")).unwrap().to_string();
    let xml = String::from_utf8_lossy(base.parts().bytes(&slide).unwrap()).into_owned();
    let hostile = xml.replacen("</p:spTree>", &format!("{body}</p:spTree>"), 1);
    assert_ne!(xml, hostile);
    let mut deck = Deck::open(&deck_bytes("0003"), Limits::default()).unwrap();
    deck.set_part(&slide, Arc::from(hostile.as_bytes()));
    let n = deck.presentation().unwrap().slides.iter().position(|s| s.target.as_deref() == Some(slide.as_str())).unwrap();
    assert!(deck.slide(u32::try_from(n).unwrap()).is_err(), "the depth limit must refuse a 10,000-deep group");
}
