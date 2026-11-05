//! Debug helper of the D5b parity (feature `odf`): the Rust side of `js/odf-dump.mjs`.
//!
//! ```text
//! cargo run -p uo-docx --features odf --example odf_dump -- docx <file.docx> <part>
//! cargo run -p uo-docx --features odf --example odf_dump -- docx <inputs-file> <key> <part>
//! cargo run -p uo-docx --features odf --example odf_dump -- trace <key>        readable canonical trace of read_odt
//! ```
//!
//! `docx` prints one part of the `.odt` written for the docx (`content.xml`, `styles.xml`…); `trace`
//! prints the canonical stream of the model `read_odt` gives for a generated package of
//! `tests/parity/odf.inputs` (the TS side: `node js/odf-dump.mjs trace <key>`), to diff the first
//! diverging item.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr, clippy::print_stdout)]

#[cfg(not(feature = "odf"))]
fn main() {
    eprintln!("build with --features odf");
}

#[cfg(feature = "odf")]
fn main() {
    use std::io::Write;
    use std::path::Path;

    use uo_docx::DocxLimits;
    use uo_docx::canon::{CanonSink, CanonWriter, canon_boxed};
    use uo_docx::js::JsValue;
    use uo_docx::odf::{OdtOptions, read_odt, write_odt};
    use uo_docx::read::{ReadOptions, read_document};
    use uo_opc::package::{OpenOptions, SaveOptions};
    use uo_opc::{Limits, Package};
    use uo_testkit::Stream;

    struct Sink(Stream);
    impl CanonSink for Sink {
        fn tag(&mut self, c: u8) {
            self.0.tag(c);
        }
        fn u32(&mut self, v: u32) {
            self.0.u32(v);
        }
        fn f64(&mut self, v: f64) {
            self.0.f64(v);
        }
        fn bool(&mut self, v: bool) {
            self.0.bool(v);
        }
        fn bytes(&mut self, b: &[u8]) {
            self.0.bytes(b);
        }
        fn digest_of(&mut self, b: &[u8]) {
            self.0.digest_of(b);
        }
    }

    fn unescape(line: &str) -> String {
        let mut out = String::new();
        let mut it = line.chars();
        while let Some(c) = it.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match it.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(o) => out.push(o),
                None => {}
            }
        }
        out
    }
    fn unhex(s: &str) -> Vec<u8> {
        s.as_bytes().chunks(2).map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap()).collect()
    }
    /// The parts of a package of an inputs file.
    fn inputs(file: &str, key: &str) -> (Vec<(String, Vec<u8>)>, Option<Vec<u8>>) {
        let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity").join(file)).unwrap();
        let mut on = false;
        let (mut parts, mut raw) = (Vec::new(), None);
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            if let Some(k) = line.strip_prefix("@input ") {
                if on {
                    break;
                }
                on = k == key;
            } else if !on {
                continue;
            } else if let Some(h) = line.strip_prefix("@raw ") {
                raw = Some(unhex(h));
            } else if let Some((name, data)) = line.split_once('\t') {
                let bytes = data.strip_prefix("@hex:").map_or_else(|| unescape(data).into_bytes(), unhex);
                parts.push((name.to_owned(), bytes));
            }
        }
        (parts, raw)
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["docx", file, part] if Path::new(file).extension().is_some_and(|e| e == "docx") => {
            let bytes = std::fs::read(file).unwrap();
            let pkg = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
            let doc = read_document(&pkg, &ReadOptions::default()).unwrap();
            let odt = write_odt(&doc, &OdtOptions::default()).unwrap();
            let out = Package::open(&odt, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
            std::io::stdout().write_all(out.part(part).unwrap().unwrap_or(&[])).unwrap();
        }
        ["docx", file, key, part] => {
            let (parts, _) = inputs(file, key);
            let mut base = Package::new_empty(&Limits::DEFAULT, &OpenOptions::default());
            let ct = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
            let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
            base.set_part_text("[Content_Types].xml", ct);
            base.set_part_text("_rels/.rels", rels);
            if parts.iter().any(|(p, _)| p == "word/document.xml") {
                base.set_part_text(
                    "word/document.xml",
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body/></w:document>",
                );
            }
            for (p, d) in parts {
                base.set_part(p, d);
            }
            let doc = read_document(&base, &ReadOptions::default()).unwrap();
            let odt = write_odt(&doc, &OdtOptions::default()).unwrap();
            let out = Package::open(&odt, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
            std::io::stdout().write_all(out.part(part).unwrap().unwrap_or(&[])).unwrap();
        }
        ["trace", key] => {
            let (parts, raw) = inputs("odf.inputs", key);
            let bytes = raw.unwrap_or_else(|| {
                Package::from_parts(parts, &Limits::DEFAULT, &OpenOptions::default())
                    .save(&SaveOptions { stored: vec!["mimetype".to_owned()], ..SaveOptions::default() })
                    .unwrap()
            });
            let doc = read_odt(&bytes, &DocxLimits::DEFAULT).unwrap();
            let mut sink = Sink(Stream::traced());
            {
                let mut w = CanonWriter::new(&mut sink).with_media(&doc.media);
                canon_boxed(
                    &mut w,
                    vec![
                        ("blocks", Box::new(|w| doc.body.canon(w))),
                        (
                            "sections",
                            Box::new(|w| {
                                w.array(doc.sections.len());
                                for s in doc.sections.iter() {
                                    // the TS canon (js/canon.mjs `sections[]`) writes `@views` on every section; an ODF
                                    // section has no header/footer stories, so both views are the same (empty): "synced"
                                    uo_docx::js::object_canon_with(s, &[("@views", &|w| w.str("synced"))], w);
                                }
                            }),
                        ),
                        ("comments", Box::new(|w| doc.comments.canon(w))),
                        ("styles", Box::new(|w| doc.styles.canon(w))),
                    ],
                );
            }
            print!("{}", sink.0.trace());
        }
        _ => eprintln!("usage: see the header of examples/odf_dump.rs"),
    }
}
