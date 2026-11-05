//! The OfficeArt inputs shared by the odraw memory and no-panic tests: the `.doc` fixtures (table,
//! WordDocument and Data streams, fcDggInfo/lcbDggInfo as the parity capture records them) and
//! the generated contents of the capture.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_testkit::Capture;

pub const CAPTURE: &str = include_str!("../captures/odraw.capture");

/// One document's OfficeArt inputs.
pub struct Doc {
    pub key: String,
    pub table: Vec<u8>,
    pub fc: u32,
    pub lcb: u32,
    pub word: Option<Vec<u8>>,
    pub data: Option<Vec<u8>>,
}

pub fn unhex(h: &str) -> Vec<u8> {
    let v = |c: u8| match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => panic!("bad hex"),
    };
    h.as_bytes().chunks(2).map(|p| (v(p[0]) << 4) | v(p[1])).collect()
}

/// The fixtures and the generated documents of the capture.
pub fn docs() -> Vec<Doc> {
    let cap = Capture::parse(CAPTURE).unwrap();
    let dir = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR")).join("write/packages/docx-core/test/fixtures/doc");
    let mut out = Vec::new();
    for r in cap.records() {
        if let Some(name) = r.key.strip_prefix("doc/") {
            let bytes = std::fs::read(dir.join(format!("{name}.doc"))).unwrap();
            let cfb = uo_opc::cfb::CfbFile::open(&bytes).unwrap();
            let mut it = r.get("src").unwrap().split(':');
            let tname = it.next().unwrap();
            let (fc, lcb) = (it.next().unwrap().parse().unwrap(), it.next().unwrap().parse().unwrap());
            out.push(Doc {
                key: r.key.clone(),
                table: cfb.stream(tname).unwrap().unwrap(),
                fc,
                lcb,
                word: cfb.stream("WordDocument").unwrap(),
                data: cfb.stream("Data").unwrap(),
            });
        } else if r.key.starts_with("gen/") {
            let data = r.get("data").unwrap();
            out.push(Doc {
                key: r.key.clone(),
                table: unhex(r.get("hex").unwrap()),
                fc: r.get("fc").unwrap().parse().unwrap(),
                lcb: r.get("lcb").unwrap().parse().unwrap(),
                word: Some(unhex(r.get("word").unwrap())),
                data: (data != "-").then(|| unhex(data)),
            });
        }
    }
    out
}

/// The single BLIP records and FOPT blocks of the capture.
pub fn records(prefix: &str) -> Vec<Vec<u8>> {
    let cap = Capture::parse(CAPTURE).unwrap();
    cap.records().iter().filter(|r| r.key.starts_with(prefix)).map(|r| unhex(r.get("hex").unwrap())).collect()
}
