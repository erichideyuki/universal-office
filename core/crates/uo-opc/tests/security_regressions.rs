//! Regression tests of the stage-4 security review of `uo-opc` (record:
//! `core/docs/modules/uo-opc-container.md`, "Security (stage 4)"). Each test is the input that
//! showed the problem; the fixes keep every result identical to the frozen TS.
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions

use std::time::{Duration, Instant};

use uo_opc::flate;
use uo_opc::rels::{self, Relationship};
use uo_opc::xml::ParseOptions;
use uo_opc::xml::raw::{self, Kid};

/// LSB-first bit writer (DEFLATE order).
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn new() -> Self {
        Bits { out: Vec::new(), acc: 0, n: 0 }
    }
    fn put(&mut self, v: u32, bits: u32) {
        self.acc |= u64::from(v) << self.n;
        self.n += bits;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    /// A Huffman code, most significant bit first.
    fn code(&mut self, code: u32, len: u32) {
        for k in (0..len).rev() {
            self.put((code >> k) & 1, 1);
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// One dynamic block whose literal tree is over-subscribed: symbols 0..=255 have length 2,
/// 256 length 1 and 257 length 15, so fflate's `hMap` loops 2^13 times for each of the 256
/// length-2 symbols over a 32,768-entry table (2.1 million writes). Symbol 256 ends up on every
/// even index, so the block decodes as just its end-of-block code (one 0 bit).
fn oversubscribed_block(b: &mut Bits) {
    b.put(0, 1); // BFINAL
    b.put(2, 2); // dynamic
    b.put(31, 5); // HLIT: 288 codes
    b.put(31, 5); // HDIST: 32 codes
    b.put(15, 4); // HCLEN: 19 code length codes
    // CLIM order: 2, 16 and 18 get length 2; 1 and 15 length 3
    for sym in [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15] {
        b.put(
            match sym {
                2 | 16 | 18 => 2,
                1 | 15 => 3,
                _ => 0,
            },
            3,
        );
    }
    // canonical codes: 2 → 00, 16 → 01, 18 → 10, 1 → 110, 15 → 111
    b.code(0b00, 2); // literal 0: length 2
    for _ in 0..42 {
        b.code(0b01, 2); // repeat the previous length 6 times
        b.put(3, 2);
    }
    b.code(0b01, 2); // 3 more: 256 lengths of 2
    b.put(0, 2);
    b.code(0b110, 3); // 256: length 1
    b.code(0b111, 3); // 257: length 15
    b.code(0b10, 2); // 30 zeros (258..=287)
    b.put(19, 7);
    b.code(0b10, 2); // 32 zero distance lengths
    b.put(21, 7);
    b.put(0, 1); // end of block
}

fn oversubscribed_stream(blocks: usize) -> Vec<u8> {
    let mut b = Bits::new();
    for _ in 0..blocks {
        oversubscribed_block(&mut b);
    }
    // a final empty fixed block: BFINAL, type 1, end-of-block (7 zero bits)
    b.put(1, 1);
    b.put(1, 2);
    b.put(0, 7);
    b.finish()
}

/// F1: building the decoding tables of over-subscribed Huffman trees was O(symbols × 2^15) per
/// block: 35 bytes of input cost ~2.1 million table writes, so a 1 MB part kept the inflater
/// busy for minutes (the TS fflate has the same loop). The table is now built in O(2^15 +
/// symbols) with exactly the same contents (unit test `h_map_matches_the_literal_port`).
#[test]
fn oversubscribed_trees_inflate_in_linear_time() {
    let data = oversubscribed_stream(3_000);
    assert!(data.len() < 110_000, "{} bytes", data.len());
    let t = Instant::now();
    let out = flate::inflate_raw(&data, None, 1 << 20).unwrap();
    let took = t.elapsed();
    assert!(out.is_empty());
    // before the fix: 6.3 billion table writes
    assert!(took < Duration::from_secs(10), "took {took:?}");
}

/// F2: `remove_rels` rebuilt the whole text once per removed relationship (O(n²)): removing
/// 80,000 relationships of a 5.4 MB part copied ~200 GB (about 30 s in release).
#[test]
fn remove_rels_is_linear() {
    let n = 80_000;
    let list: Vec<Relationship> = (0..n).map(|i| Relationship::new(&format!("rId{i}"), "t", &format!("media/image{i}.png"))).collect();
    let xml = rels::write_rels(&list, "");
    let t = Instant::now();
    let (out, removed) = rels::remove_rels(&xml, &ParseOptions::LENIENT, |r| r.id != "rId7").unwrap();
    assert!(t.elapsed() < Duration::from_secs(10), "took {:?}", t.elapsed());
    assert_eq!(removed.len(), n - 1);
    assert_eq!(out, rels::write_rels(&list[7..8], ""));
}

/// F3: `first_free_rel_id` compared every candidate with every id (O(n²); 150,000 ids took about
/// 20 s in release).
#[test]
fn first_free_rel_id_is_linear() {
    let n = 150_000;
    let list: Vec<Relationship> = (1..=n).map(|i| Relationship::new(&format!("rId{i}"), "t", "x")).collect();
    let t = Instant::now();
    assert_eq!(rels::first_free_rel_id(&list, "rId", 1), format!("rId{}", n + 1));
    assert!(t.elapsed() < Duration::from_secs(10), "took {:?}", t.elapsed());
    // a start at the top of the range does not overflow
    let top = vec![Relationship::new(&format!("rId{}", u64::MAX), "t", "x")];
    assert_eq!(rels::first_free_rel_id(&top, "rId", u64::MAX), "rId0");
}

/// F4: `set_kid` removed repeated kids one `Vec::remove` at a time (O(n²) moves; 250,000 kids took
/// about 30 s in release).
#[test]
fn set_kid_with_many_duplicates_is_linear() {
    let n = 250_000;
    let mut kids: Vec<Kid> =
        (0..n).map(|i| Kid { name: if i % 2 == 0 { "w:b".into() } else { "w:i".into() }, raw: format!("<k{i}/>"), el: None }).collect();
    let t = Instant::now();
    raw::set_kid(&mut kids, &[], "w:b", Some("<w:b/>"));
    assert!(t.elapsed() < Duration::from_secs(10), "took {:?}", t.elapsed());
    assert_eq!(kids.len(), n / 2 + 1);
    assert_eq!(kids[0].raw, "<w:b/>");
    assert!(kids[1..].iter().all(|k| k.name == "w:i"));
    assert_eq!(kids[1].raw, "<k1/>");
    assert_eq!(kids[n / 2].raw, format!("<k{}/>", n - 1));
}

/// F5: `expand_self_closing` searched for the next `>` from every `<root` (O(n²) when the text
/// has many of them before a `>`, or none: 3 MB took 22 s in release).
#[test]
fn expand_self_closing_is_linear() {
    let mut xml = "<Relationships ".repeat(200_000);
    let t = Instant::now();
    assert_eq!(raw::expand_self_closing(&xml, "Relationships"), xml);
    xml.push_str("/>");
    let out = raw::expand_self_closing(&xml, "Relationships");
    assert!(t.elapsed() < Duration::from_secs(10), "took {:?}", t.elapsed());
    // the regex `/<root\b([^>]*?)\s*\/>/` matches at the FIRST `<root`, spanning the others
    assert!(out.starts_with("<Relationships <Relationships "), "{}", &out[..40]);
    assert!(out.ends_with("<Relationships></Relationships>"));
    // and the result is the same as before for ordinary input
    assert_eq!(raw::expand_self_closing("<a/><Relationships x=\"1\" />", "Relationships"), "<a/><Relationships x=\"1\"></Relationships>");
    assert_eq!(
        raw::expand_self_closing("<Relationships><Relationships/>", "Relationships"),
        "<Relationships><Relationships></Relationships>"
    );
}

/// F6 (found by the `xml_lenient` fuzz target): with `keep_comments`/`keep_pis` in a lenient
/// preset, `<!-->` and `<?>` underflowed `end - start` (a panic in debug builds, a limit error
/// in release). The TS reads them as an empty comment/instruction (`indexOf` from the `<`).
#[test]
fn lenient_empty_comment_and_pi_shapes() {
    use uo_opc::xml::{ContentPi, Node, parse};
    let opts = ParseOptions { keep_comments: true, keep_pis: true, content_pi: ContentPi::Skip, ..ParseOptions::LENIENT };
    let doc = parse("<a><!--><?><b/></a>", &opts).unwrap();
    let kids: Vec<Node<'_>> = doc.root().children().collect();
    assert!(matches!(kids.as_slice(), [Node::Comment(""), Node::Pi(""), Node::Element(_)]), "{kids:?}");
    // the TS presets (comments dropped) are unchanged
    let doc = parse("<a><!--><b/></a>", &ParseOptions::LENIENT).unwrap();
    assert_eq!(doc.root().child_count(), 1);
}

/// F7: inflate allocations the host cannot make return `Error::Limit` instead of aborting the
/// process (`handle_alloc_error`) or panicking (`capacity overflow` above `isize::MAX`, which on
/// wasm32 is below the default 2 GiB binary part cap). The input was an empty stream declaring
/// 4 EiB with no cap (the S1 path; before F7 the test process aborted). Since D1 that stream is
/// `Corrupt` before any allocation; the output growth keeps `try_reserve` (F7), which only an
/// output above the host's memory reaches.
#[test]
fn impossible_inflate_allocation_is_an_error() {
    let r = flate::inflate_raw(&[], Some(1 << 62), u64::MAX);
    assert!(matches!(r, Err(uo_opc::Error::Corrupt("empty deflate stream with a declared size"))), "{r:?}");
    assert!(flate::inflate_raw(&[], Some(5), 10).is_err());
}
