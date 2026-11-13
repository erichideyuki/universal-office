//! Parity replay of the numeric functions of `uo-jsnum` (increment A, everything but pow)
//! against `tests/captures/jsnum.capture`, written by `js/capture-jsnum.mjs` in Node 24.15.0
//! (V8 13.6.233.17, x64). The inputs are regenerated with the same PRNG and the same IEEE
//! operations as the script; the outputs (NaN canonicalized) are hashed with the same canonical
//! stream. 10^5 samples per distribution run in `cargo test`; 10^7 with `--include-ignored`.
//! On a mismatch, run the script with `UO_JSNUM_DUMP=<key>` and this test prints the first 20
//! differing inputs of that record.
#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use support::{Rec, SMALL, capture, hex, inputs, make_rng, numeric_records, op};
use uo_jsnum::conv::canonicalize_nan;
use uo_testkit::{Replay, Stream};

struct Digests {
    small: String,
    full: Option<String>,
}

fn run(rec: &Rec, total: usize) -> Digests {
    let specials = inputs().specials;
    let mut rng = make_rng(&rec.key);
    let mut s = Stream::new();
    let f = op(rec.op);
    let mut args = vec![0.0; rec.gens.len()];
    let mut small = None;
    for i in 0..total {
        for (a, g) in args.iter_mut().zip(&rec.gens) {
            *a = g.draw(&mut rng, &specials);
        }
        s.f64(canonicalize_nan(f(&args)));
        if i + 1 == SMALL {
            small = Some(s.digest().short());
        }
    }
    Digests { small: small.unwrap_or_default(), full: (total > SMALL).then(|| s.finish().short()) }
}

fn parallel<T: Send>(recs: &[Rec], job: impl Fn(&Rec) -> T + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism().map_or(4, usize::from).min(16);
    let mut out: Vec<Option<T>> = (0..recs.len()).map(|_| None).collect();
    let chunks: Vec<Vec<(usize, &Rec)>> =
        (0..threads).map(|t| recs.iter().enumerate().filter(|(i, _)| i % threads == t).collect()).collect();
    let job = &job;
    let results: Vec<Vec<(usize, T)>> = std::thread::scope(|sc| {
        let handles: Vec<_> =
            chunks.into_iter().map(|chunk| sc.spawn(move || chunk.into_iter().map(|(i, r)| (i, job(r))).collect::<Vec<_>>())).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for (i, v) in results.into_iter().flatten() {
        out[i] = Some(v);
    }
    out.into_iter().map(Option::unwrap).collect()
}

fn dump_report(rec: &Rec) {
    let Ok(wanted) = std::env::var("UO_JSNUM_DUMP") else { return };
    if wanted != rec.key {
        return;
    }
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/jsnum-dump").join(format!("{}.f64", rec.key.replace('/', "_")));
    let Ok(bytes) = std::fs::read(&path) else { return eprintln!("no dump at {}", path.display()) };
    let width = rec.gens.len() + 1;
    let values: Vec<f64> = bytes.as_chunks::<8>().0.iter().map(|c| f64::from_le_bytes(*c)).collect();
    let specials = inputs().specials;
    let mut rng = make_rng(&rec.key);
    let f = op(rec.op);
    let mut shown = 0;
    for sample in values.chunks_exact(width) {
        let args: Vec<f64> = rec.gens.iter().map(|g| g.draw(&mut rng, &specials)).collect();
        let got = canonicalize_nan(f(&args));
        let want = sample[width - 1];
        if hex(got) != hex(want) {
            let shown_args: Vec<String> = args.iter().map(|a| hex(*a)).collect();
            eprintln!("{} {} => rust {} node {}", rec.key, shown_args.join(" "), hex(got), hex(want));
            shown += 1;
            if shown == 20 {
                break;
            }
        }
    }
}

fn replay(full: bool) {
    let cap = capture();
    let cap = cap.subset(|k| k.starts_with("num/"));
    let all = numeric_records();
    let digests = parallel(&all, |rec| {
        let total = if full { cap.get(&format!("num/{}", rec.key), "n_full").unwrap().parse::<usize>().unwrap() } else { SMALL };
        run(rec, total)
    });
    let mut replay = Replay::new(&cap);
    for (rec, d) in all.iter().zip(&digests) {
        let key = format!("num/{}", rec.key);
        let seed = format!("{:x}", support::fnv(&rec.key));
        replay.check(&key, "seed", &seed);
        replay.skip(&key, "n_small");
        let ok_small = replay.check(&key, "small", &d.small);
        if full {
            replay.skip(&key, "n_full");
            let ok_full = replay.check(&key, "full", d.full.as_deref().unwrap_or(""));
            if !ok_full {
                dump_report(rec);
            }
        } else {
            replay.skip(&key, "n_full");
            replay.skip(&key, "full");
            if !ok_small {
                dump_report(rec);
            }
        }
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}

#[test]
fn digests_at_10_5_samples_match_node() {
    replay(false);
}

#[test]
#[ignore = "10^7 samples per distribution (about a minute in release): cargo test -p uo-jsnum --release -- --include-ignored"]
fn digests_at_10_7_samples_match_node() {
    replay(true);
}

#[test]
fn every_record_of_the_table_is_in_the_capture_and_back() {
    let cap = capture();
    let keys: Vec<String> = numeric_records().iter().map(|r| format!("num/{}", r.key)).collect();
    let in_cap: Vec<&str> = cap.records().iter().map(|r| r.key.as_str()).filter(|k| k.starts_with("num/")).collect();
    assert_eq!(keys.len(), in_cap.len());
    for k in &keys {
        assert!(cap.record(k).is_some(), "{k} is not in the capture");
    }
}
