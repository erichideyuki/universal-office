//! Parity of the pure model files of D2b-F3 against the frozen TS: replays
//! `tests/parity/d2b-f3-ops.capture`, written by `js/capture-d2b-f3-ops.mjs` — `table-ops.ts`
//! (`TOps`), `table-sort.ts` (with the editor's `compareText`) and `table-formula.ts` over the
//! tables of corpus documents, with the JSON round trip and `relink` of an undo step after every op.
//!
//! Facets per op: `ret` (what the op returned), `tbl` (the canonical digest of the table), `json`
//! (the digest of its JSON round trip). Documents whose tables hold lone surrogates from character
//! references (R1's A3) are not in the capture's corpus filter: none is compared differently.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f3-ops.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f3_support;
#[allow(unused_imports)]
use crate::support;

use std::path::Path;

use d2b_f3_support::ops_codes::{canon_digest, json_digest, make_synthetic, run};
use uo_docx::read::{ReadOptions, read_document};
use uo_opc::Package;
use uo_opc::package::OpenOptions;
use uo_testkit::{Capture, Replay, repo_root};

#[test]
fn d2b_f3_model_files_match_the_ts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2b-f3-ops.capture")).unwrap();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let ops: usize = cap.meta("ops").unwrap().parse().unwrap();
    let mut replay = Replay::new(&cap);
    let mut tables = 0;
    let mut steps = 0;
    for rec in cap.records().iter().filter(|r| r.key.ends_with("#init")) {
        let base = rec.key.trim_end_matches("#init");
        let (rel, tpart) = base.rsplit_once("#t").unwrap();
        let ti: usize = tpart.parse().unwrap();
        let (mut ids, media, mut t) = if rel == "syn" {
            let mut ids = uo_docx::ids::IdAlloc::new();
            replay.check(&rec.key, "build", rec.get("build").unwrap());
            let t = make_synthetic(rec.get("build").unwrap(), &mut ids);
            (ids, uo_docx::model::MediaStore::default(), t)
        } else {
            let bytes = std::fs::read(root.join(rel)).unwrap();
            let pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).unwrap();
            let doc = read_document(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() }).unwrap();
            (doc.ids.clone(), doc.media.clone(), (*doc.body.tables.get(ti).unwrap().table).clone())
        };
        replay.check(&rec.key, "tbl", &canon_digest(&t, &media));
        replay.check(&rec.key, "json", &json_digest(&t, &mut ids, &media));
        for j in 0..ops {
            let key = format!("{base}#{j}");
            let Some(code) = cap.get(&key, "op") else { break };
            replay.check(&key, "op", code);
            let ret = run(code, &mut t, &mut ids, &media);
            replay.check(&key, "ret", &ret);
            if std::env::var("UO_F3_DUMP").is_ok_and(|d| d == key) {
                println!("RUST {}", uo_docx::js::JsonWriter::stringify(&t, Some(&media)).unwrap());
            }
            if ret == "err" {
                break;
            }
            steps += 1;
            if t.rows.is_empty() {
                replay.check(&key, "tbl", "empty");
                replay.check(&key, "json", "empty");
                break;
            }
            replay.check(&key, "tbl", &canon_digest(&t, &media));
            replay.check(&key, "json", &json_digest(&t, &mut ids, &media));
        }
        tables += 1;
    }
    let report = replay.finish();
    println!("d2b-f3-ops: {tables} tables, {steps} ops, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_D2A_ALL").map_or(40, |_| 100_000)));
    }
    report.assert_clean();
    assert!(tables > 100);
}
