//! Scenarios of the uo-opc benchmark (rust-port stage 5, `tools/bench/run.mjs --suites opc`),
//! shared by the native runner (`examples/opc_bench.rs`) and the wasm one (`uo-wasm` feature
//! `opc-bench`); the TS side is `tools/bench/opc/ts.mjs`, with the same operations on the frozen
//! 0.9.x packages. Memory is counted by `uo-alloc` (allocated bytes and peak live bytes above the
//! start of the operation); the counts are what the runners print next to the times.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use uo_alloc::{Usage, measure};
use uo_opc::Limits;
use uo_opc::flate;
use uo_opc::limits::is_xml_like;
use uo_opc::package::{Package, SaveOptions};
use uo_opc::zip;

use crate::memory_support::{Kind, open_options, preset};

/// What a pass read: parts, elements of the parsed XML parts, bytes of the other parts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub docs: u64,
    pub parts: u64,
    pub elements: u64,
    pub bytes: u64,
}

impl Totals {
    fn add(&mut self, o: Totals) {
        self.docs += o.docs;
        self.parts += o.parts;
        self.elements += o.elements;
        self.bytes += o.bytes;
    }
}

/// Memory of a pass: bytes allocated in total, the highest peak of one operation, and what the
/// last operation kept (outputs alive).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cost {
    pub allocated: u64,
    pub peak: i64,
    pub kept: i64,
}

impl Cost {
    fn add(&mut self, u: Usage) {
        self.allocated += u.allocated;
        self.peak = self.peak.max(u.peak_above_start);
        self.kept = u.live_delta;
    }
}

/// Every XML-like part parsed with the app's preset through `xml_part` (the format crates' way,
/// gate 2 D3), every other part read.
pub fn read_and_parse(pkg: &Package, kind: Kind) -> Totals {
    let mut t = Totals { docs: 1, ..Totals::default() };
    for name in pkg.part_names() {
        t.parts += 1;
        if is_xml_like(name) {
            t.elements += pkg.xml_part(name, &preset(kind, name)).unwrap().element_count() as u64;
        } else {
            t.bytes += pkg.part(name).unwrap().unwrap().len() as u64;
        }
    }
    t
}

/// (a) open + read + parse of each package, each measured on its own (the package dropped).
pub fn corpus(files: &[&[u8]], kind: Kind) -> (Totals, Cost) {
    let mut totals = Totals::default();
    let mut cost = Cost::default();
    for bytes in files {
        let (t, u) = measure(|| {
            let pkg = Package::open(bytes, &Limits::DEFAULT, &open_options(kind)).unwrap();
            read_and_parse(&pkg, kind)
        });
        totals.add(t);
        cost.add(u);
    }
    (totals, cost)
}

/// (b) open + read + parse of one package, kept alive for `kept` (the trees are dropped per part).
pub fn package(bytes: &[u8], kind: Kind) -> (Totals, Cost) {
    let ((pkg, t), u) = measure(|| {
        let pkg = Package::open(bytes, &Limits::DEFAULT, &open_options(kind)).unwrap();
        let t = read_and_parse(&pkg, kind);
        (pkg, t)
    });
    let mut cost = Cost::default();
    cost.add(u);
    drop(pkg);
    (t, cost)
}

/// (c) the entry `name` of an archive inflated with its declared size (`unzipSync` with a
/// filter in the TS): output bytes.
pub fn inflate_entry(archive: &[u8], name: &str) -> (u64, Cost) {
    let (n, u) = measure(|| {
        let e = zip::read(archive, u32::MAX).unwrap().into_iter().find(|e| e.name == name).unwrap();
        flate::inflate_raw(e.compressed(archive), Some(e.size), u64::MAX).unwrap().len() as u64
    });
    let mut cost = Cost::default();
    cost.add(u);
    (n, cost)
}

/// (d) the edit of the TS budgets (`ts_budgets.mjs` `edit`): an element inserted before the last
/// end tag of `main` and a new part, then the save (the package is opened before). Output bytes.
pub fn edit_and_save(pkg: &mut Package, main: &str) -> (u64, Cost) {
    let (n, u) = measure(|| {
        let text = pkg.part_text(main).unwrap().unwrap().into_owned();
        let i = text.rfind("</").unwrap();
        let edited = [&text[..i], "<uo:soak xmlns:uo=\"urn:uo\" n=\"1\"/>", &text[i..]].concat();
        pkg.set_part_text(main, edited);
        pkg.set_part_text("customXml/soak.xml", "<soak n=\"1\"/>");
        pkg.save(&SaveOptions::default()).unwrap().len() as u64
    });
    let mut cost = Cost::default();
    cost.add(u);
    (n, cost)
}

/// The corpus as the wasm runner passes it: each file as a little-endian u32 length and its bytes.
pub fn frames(buf: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 4 <= buf.len() {
        let n = u32::from_le_bytes(buf[at..at + 4].try_into().unwrap()) as usize;
        out.push(&buf[at + 4..at + 4 + n]);
        at += 4 + n;
    }
    out
}

pub fn kind_of(k: u32) -> Kind {
    match k {
        0 => Kind::Docx,
        1 => Kind::Xlsx,
        _ => Kind::Pptx,
    }
}
