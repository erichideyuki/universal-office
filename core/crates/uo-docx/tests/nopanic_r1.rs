//! No-panic loops of frontier R1: every public reader of `read::props` and `read::text_effects`
//! over the quirk corpus, seeded mutations of it (a character replaced, removed, duplicated; a
//! junk string inserted), and random numeric strings. A panic fails the test; the result is not
//! looked at (the parity tests look at it).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::read::DOCX;
use uo_docx::read::props::{
    int, measure_twips, on_off_val, read_border, read_ppr, read_revision_info, read_rpr, read_shading, read_tbl_pr, read_tc_pr, read_tr_pr,
    split_measure,
};
use uo_docx::read::text_effects::{element_xml, read_color, read_fill, read_text_effects};
use uo_opc::xml::{Document, parse};

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        s
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
}

fn read_all(d: &Document) -> usize {
    let root = d.root();
    let mut n = 0;
    for el in std::iter::once(root).chain(root.descendants()) {
        n += usize::from(read_rpr(Some(el)).is_some());
        n += usize::from(read_ppr(Some(el)).is_some());
        n += usize::from(read_tbl_pr(Some(el)).is_some());
        n += usize::from(read_tr_pr(Some(el)).is_some());
        n += usize::from(read_tc_pr(Some(el)).is_some());
        n += usize::from(read_border(Some(el)).is_some());
        n += usize::from(read_shading(Some(el)).is_some());
        n += usize::from(read_color(Some(el)).is_some());
        n += usize::from(read_fill(Some(el)).is_some());
        n += read_text_effects(el).0.iter().count();
        n += element_xml(el).len();
        n += read_revision_info(el).author.len();
        n += usize::from(on_off_val(el));
    }
    n
}

fn corpus() -> Vec<String> {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/parity/props.quirks.txt")).unwrap();
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split_once('\t').unwrap().1.replace("\\n", "\n").replace("\\r", "\r").replace("\\t", "\t").replace("\\\\", "\\"))
        .collect()
}

#[test]
fn corpus_and_mutations_do_not_panic() {
    let texts = corpus();
    let mut rng = Rng(0x00c0_ffee);
    let mut parsed = 0;
    let junk = [
        "",
        "-",
        ".",
        "NaN",
        "1e999",
        "99999999999999999999999999",
        "\u{0}",
        "%",
        "%%",
        "12pt%",
        "ÿ",
        "𝒜",
        " ",
        "0x",
        "-0x1",
        "1e",
        "Infinity",
    ];
    for t in &texts {
        if let Ok(d) = parse(t, &DOCX) {
            read_all(&d);
            parsed += 1;
        }
    }
    for i in 0..6000 {
        let base = &texts[i % texts.len()];
        let mut s: Vec<char> = base.chars().collect();
        for _ in 0..1 + rng.below(4) {
            let at = rng.below(s.len());
            match rng.below(4) {
                0 => {
                    s.remove(at);
                }
                1 => {
                    let c = s[at];
                    s.insert(at, c);
                }
                2 => s[at] = char::from_u32(rng.next() % 0x3000).unwrap_or('x'),
                _ => {
                    let j = junk[rng.below(junk.len())];
                    for (k, c) in j.chars().enumerate() {
                        s.insert(at + k, c);
                    }
                }
            }
        }
        let m: String = s.into_iter().collect();
        if let Ok(d) = parse(&m, &DOCX) {
            read_all(&d);
            parsed += 1;
        }
    }
    assert!(parsed > 3000, "{parsed} parsed");
}

#[test]
fn numeric_helpers_do_not_panic() {
    let mut rng = Rng(7);
    let alphabet: Vec<char> = "0123456789.-+eExXpPtTmMcCiInN % \t\u{a0}\u{feff}９ß𝒜".chars().collect();
    for _ in 0..20000 {
        let len = rng.below(12);
        let s: String = (0..len).map(|_| alphabet[rng.below(alphabet.len())]).collect();
        let _ = (int(Some(&s)), measure_twips(Some(&s)), split_measure(&s));
    }
}
