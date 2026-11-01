//! Differential test against the frozen TS parsers: every line of `xml_support/ts_oracle.txt`
//! (written by `xml_support/gen_ts_oracle.mjs` from the 0.9.x dist) is replayed with the
//! matching preset and the trees (or the failure) must be identical.
//!
//! Accepted differences, by decision of gate 1 (G5):
//! - sheet-core loops forever (HANG) or reads garbage on an unclosed `<!--`, `<![CDATA[` or
//!   `</x`: an `Unclosed` error here — accepted when slides-core (which already fails there)
//!   also failed on the same input;
//! - `&constructor;` and the other `Object.prototype` names stay literal (sheet-core and
//!   slides-core `ENTITIES[e]` returned the prototype members; docx-core already used `hasOwn`);
//! - lone surrogates are U+FFFD (the generator writes them so; decision in the record).
#![allow(clippy::unwrap_used, clippy::panic, clippy::type_complexity)] // helpers outside #[test] functions

mod xml_support;

use std::collections::HashMap;
use uo_opc::error::{Error, XmlErrorKind};
use uo_opc::xml::parse;
use xml_support::{dump, preset, unq};

#[test]
fn trees_match_the_ts_parsers() {
    let text = include_str!("xml_support/ts_oracle.txt");
    let lines: Vec<(&str, String, &str)> = text
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let mut it = l.splitn(3, '\t');
            let p = it.next().unwrap();
            let input = unq(it.next().unwrap());
            (p, input, it.next().unwrap())
        })
        .collect();
    let slides_failed: HashMap<&str, bool> = lines.iter().filter(|l| l.0 == "slides").map(|l| (l.1.as_str(), l.2 == "ERR")).collect();
    let mut mismatches = Vec::new();
    let (mut ok, mut errs, mut g5) = (0, 0, 0);
    for (p, input, expected) in &lines {
        let opts = preset(p);
        let got = parse(input.as_str(), &opts);
        match (&got, *expected) {
            (Err(_), "ERR") => errs += 1,
            (Err(Error::Xml { kind: XmlErrorKind::Unclosed, .. }), "HANG") => g5 += 1,
            (Err(Error::Xml { kind: XmlErrorKind::Unclosed, .. }), _)
                if p.starts_with("sheet") && slides_failed.get(input.as_str()) == Some(&true) =>
            {
                g5 += 1
            }
            (Ok(doc), exp) => {
                let d = dump(doc.root(), *p == "docx-pos");
                if d == exp {
                    ok += 1;
                } else if (exp.contains("[native code]") || exp.contains("[object Object]")) && d.contains("&constructor;") {
                    g5 += 1;
                } else {
                    mismatches.push(format!("{p} {input:?}\n  ts:   {exp}\n  rust: {d}"));
                }
            }
            (Err(e), exp) => mismatches.push(format!("{p} {input:?}\n  ts:   {exp}\n  rust: {e:?}")),
        }
    }
    println!("{} lines: {ok} equal trees, {errs} errors on both sides, {g5} G5 fixes (sheet hang, prototype entities)", lines.len());
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches.iter().take(25).cloned().collect::<Vec<_>>().join("\n")
    );
    assert!(ok > 1000 && errs > 500, "corpus too small: {ok} trees, {errs} errors");
}
