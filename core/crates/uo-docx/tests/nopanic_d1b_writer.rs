//! No panic in the D1b writer core on hostile input: corpus documents with their main, header,
//! settings, numbering, comments, content-types and relationship parts mutated (bytes flipped,
//! cut, tags duplicated, `>` removed, attributes doubled), read with sources, installed and saved in
//! both header/footer variants; every save that succeeds must reopen. The writer's matchers over
//! large hostile texts finish in linear time.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use uo_docx::read::{ReadOptions, read_document};
use uo_docx::write::GenNumIds;
use uo_docx::write::matchers::{code_parts, drop_comment_marks, field_code_groups, grid_of, remove_elements_lazy, replace_txbx_contents};
use uo_docx::write::parts::{comment_elements, merge_numbering, with_namespaces};
use uo_docx::write::roundtrip::{DirtyHf, install, save};
use uo_docx::write::sig::SigMode;
use uo_opc::Package;
use uo_opc::package::{OpenOptions, SaveOptions};
use uo_testkit::generate::Rng;
use uo_testkit::repo_root;

fn corpus() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|e| e == "docx") {
                out.push(p);
            }
        }
    }
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    walk(&root.join("write/packages/docx-layout/test/oracle"), &mut out);
    walk(&root.join("write/packages/docx-core/test/fixtures"), &mut out);
    out
}

const TARGETS: &[&str] = &[
    "word/document.xml",
    "word/document.xml",
    "word/document.xml",
    "word/header1.xml",
    "word/footer1.xml",
    "word/settings.xml",
    "word/numbering.xml",
    "word/comments.xml",
    "[Content_Types].xml",
    "word/_rels/document.xml.rels",
    "word/theme/theme1.xml",
];

fn mutate(rng: &mut Rng, text: &str) -> String {
    let mut s: Vec<u8> = text.as_bytes().to_vec();
    for _ in 0..1 + rng.below(4) {
        if s.is_empty() {
            break;
        }
        let at = rng.below(s.len());
        match rng.below(7) {
            0 => s[at] = b"<>/\"='&w: x"[rng.below(11)],
            1 => s.truncate(at),
            2 => {
                let end = (at + 1 + rng.below(200)).min(s.len());
                let dup = s[at..end].to_vec();
                s.splice(at..at, dup);
            }
            3 => {
                if let Some(k) = s[at..].iter().position(|&b| b == b'>') {
                    s.remove(at + k);
                }
            }
            4 => {
                let ins: &[u8] =
                    [&b" w:id=\"1\""[..], b" w:header=\"x\"", b"<w:sectPr>", b"</w:p>", b"<w:p>", b"$&", b"\xF0\x9F\x98\x80"][rng.below(7)];
                s.splice(at..at, ins.iter().copied());
            }
            5 => {
                let end = (at + rng.below(64)).min(s.len());
                s.drain(at..end);
            }
            _ => s[at] = s[at].wrapping_add(1),
        }
    }
    String::from_utf8_lossy(&s).into_owned()
}

#[test]
fn hostile_round_trips_do_not_panic() {
    let files = corpus();
    let mut rng = Rng::new(0xd1b0_bad5);
    let (mut saved, mut refused) = (0, 0);
    let limits = uo_opc::Limits::DEFAULT;
    let options = OpenOptions::default();
    for i in 0..1500 {
        let f = &files[rng.below(files.len())];
        let bytes = std::fs::read(f).unwrap();
        let Ok(mut pkg) = Package::open(&bytes, &limits, &options) else { continue };
        for _ in 0..1 + rng.below(3) {
            let target = TARGETS[rng.below(TARGETS.len())];
            if let Ok(Some(t)) = pkg.part_text(target).map(|t| t.map(std::borrow::Cow::into_owned)) {
                let m = mutate(&mut rng, &t);
                pkg.set_part_text(target, m);
            }
        }
        let Ok(mut doc) = read_document(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() }) else {
            refused += 1;
            continue;
        };
        let mut state = install(&mut doc, if i % 2 == 0 { SigMode::Canonical } else { SigMode::Ts });
        let dirty = if rng.below(2) == 0 { DirtyHf::None } else { DirtyHf::All };
        match save(&mut pkg, &doc, &mut state, dirty) {
            Ok(()) => {
                let out = pkg.save(&SaveOptions::default()).unwrap();
                Package::open(&out, &limits, &options).unwrap();
                saved += 1;
            }
            Err(_) => refused += 1,
        }
    }
    println!("no panic: {saved} hostile saves reopened, {refused} refused (read or write error)");
    assert!(saved > 500);
}

#[test]
fn matchers_are_linear_on_hostile_texts() {
    let big = |unit: &str, n: usize| unit.repeat(n);
    let cases = [
        big("<w:r>", 200_000),
        big("<w:r><w:commentReference w:id=\"9\"/>", 100_000) + "</w:r>",
        big("<w:commentRangeStart w:id=\"9\" ", 100_000) + "/>",
        big("<w:comment w:id=\"1\" ", 100_000),
        big("<w:txbxContent>", 200_000),
        big("<w:gridCol w:w=\"1\" ", 100_000),
        big("<w:trackRevisions ", 100_000) + ">",
        big("<w:numbering>", 100_000),
    ];
    for c in &cases {
        let t = Instant::now();
        let _ = drop_comment_marks(c, &["1", "2"]);
        let _ = comment_elements(c);
        let _ = replace_txbx_contents(c, |_| String::new());
        let _ = grid_of(c);
        let _ = remove_elements_lazy(c, "w:trackRevisions");
        let _ = merge_numbering(c, GenNumIds { bullet: 1001.0, decimal: 1002.0 });
        let _ = with_namespaces(c, "w:document", c);
        assert!(t.elapsed().as_secs() < 5, "{} bytes took {:?}", c.len(), t.elapsed());
    }
    // fieldCodeXml: many nested fields against a long flat instruction (V8 is O(n^k) here)
    let code = big("a{x}", 2_000) + "b";
    let flat = big("a1", 2_000) + "c";
    let t = Instant::now();
    assert_eq!(field_code_groups(&code_parts(&code), &flat), None);
    let flat_ok = big("a1", 2_000) + "b";
    assert_eq!(field_code_groups(&code_parts(&code), &flat_ok).map(|g| g.len()), Some(2_000));
    assert!(t.elapsed().as_secs() < 5);
}
