//! Speed and memory of the incremental relayout (LA7; record uo-docx-layout.md "Stage M2 —
//! incremental relayout"). Per document: a uo-docx `Session`, the caret in the middle body
//! paragraph; a recording relayout; 21 keystrokes `Type "a"` (incremental, median and p90) against
//! full layouts of the same view; then Enter, a 60-character insertion, and the memory kept.
//!   cargo build -j 4 --release -p uo-docx-layout --example bench_relayout
//!   target/.../release/examples/bench_relayout <file.docx>... [--gen 310 3130]
//! `--gen n…` adds generated documents of n paragraphs with a NUMPAGES field (two passes).
//! Prints one JSON line per case.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::print_stdout, clippy::print_stderr)]

#[path = "../tests/common/open_fonts.rs"]
mod open_fonts;

#[path = "../tests/common/docgen.rs"]
#[allow(dead_code)]
mod docgen;

#[path = "../tests/common/hyph.rs"]
mod hyph;

use std::sync::Arc;
use std::time::Instant;

use open_fonts::OpenFonts;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{ClipboardRing, Command, DocPosition, ExecCx, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;
use uo_docx_layout::LayoutEngine;
use uo_docx_layout::fonts::FontProvider;
use uo_docx_layout::incr::{IncrConfig, MemoCaps, RelayoutPath};
use uo_docx_layout::oracle::EngineOracle;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn p90(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v[(v.len() * 9 / 10).min(v.len() - 1)]
}

fn exec(s: &mut Session, oracle: &mut EngineOracle<'_>, cmd: Command) {
    let mut clip = ClipboardRing::default();
    match s.exec(cmd, &mut ExecCx { layout: oracle, clipboard: &mut clip }) {
        Ok(Outcome::Done(_)) => {}
        other => panic!("command: {other:?}"),
    }
}

/// A JSON value of an optional number (`null` when absent).
fn opt_num(v: Option<u32>) -> String {
    v.map_or_else(|| "null".to_owned(), |n| n.to_string())
}

fn report_json(name: &str, case: &str, e: &LayoutEngine, t: f64, extra: &str) {
    let r = e.last_relayout().unwrap();
    let reason = r.reason.map_or_else(|| "null".to_owned(), |w| format!("\"{w:?}\""));
    println!(
        "{{\"doc\":\"{name}\",\"case\":\"{case}\",\"ms\":{t:.2},\"path\":\"{:?}\",\"reason\":{reason},\"passes\":{},\"relaid\":[{},{}],\"resumed\":{},\"converged\":{},\"checkpoints\":{},\"spacing\":{},\"checkpoint_bytes\":{},\"retained_page_bytes\":{},\"layout_bytes\":{},\"memo\":{{\"hits\":{},\"replayed\":{},\"misses\":{},\"skipped\":{},\"unmemoizable\":{},\"over_cap\":{},\"entries\":{},\"bytes\":{}}}{extra}}}",
        r.path,
        r.passes,
        r.relaid_pages.start,
        r.relaid_pages.end,
        opt_num(r.resumed_page),
        opt_num(r.converged_page),
        r.checkpoints,
        r.spacing,
        r.checkpoint_bytes,
        r.retained_page_bytes,
        r.layout_bytes,
        r.memo.hits,
        r.memo.replayed,
        r.memo.misses,
        r.memo.skipped,
        r.memo.unmemoizable,
        r.memo.over_cap,
        r.memo.entries,
        r.memo.bytes
    );
}

fn bench(name: &str, bytes: Vec<u8>, fonts: &dyn FontProvider, reg: &Arc<uo_fonts::hyphen::HyphenationRegistry>) {
    let o = hyph::hyph_opts();
    let mut s = Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    let mut oracle = EngineOracle::new(fonts, Arc::clone(reg), hyph::hyph_opts());
    let mut eng = LayoutEngine::with_hyphenation(Arc::clone(reg)).with_incremental(IncrConfig::default());
    // warm the faces and the hyphenation tables
    drop(LayoutEngine::with_hyphenation(Arc::clone(reg)).layout(s.view(), fonts, &o).map(|l| l.pages.len()));
    let mut full = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        let n = LayoutEngine::with_hyphenation(Arc::clone(reg)).layout(s.view(), fonts, &o).unwrap().pages.len();
        full.push(ms(t));
        std::hint::black_box(n);
    }
    let full_ms = median(&mut full);
    let t = Instant::now();
    let pages = eng.relayout(s.view(), &[], fonts, &o).unwrap().pages.len();
    let rec_ms = ms(t);
    report_json(name, "recording", &eng, rec_ms, &format!(",\"pages\":{pages},\"full_ms\":{full_ms:.2}"));
    // the caret in the middle body paragraph
    let mid = u32::try_from(s.paras().unwrap().len() / 2).unwrap();
    let at = DocPosition::new(mid, 40);
    exec(&mut s, &mut oracle, Command::Text(TextCmd::SetSelection { anchor: at, focus: at }));
    let (mut inc, mut fulls) = (Vec::new(), Vec::new());
    let mut incremental = 0;
    let mut relaid_max = 0;
    for k in 0..21 {
        exec(&mut s, &mut oracle, Command::Text(TextCmd::Type { text: Utf16::from("a") }));
        let t = Instant::now();
        std::hint::black_box(eng.relayout(s.view(), &[], fonts, &o).unwrap());
        inc.push(ms(t));
        let r = eng.last_relayout().unwrap();
        if r.path == RelayoutPath::Incremental {
            incremental += 1;
        }
        relaid_max = relaid_max.max(r.relaid_pages.end - r.relaid_pages.start);
        if k % 4 == 0 {
            let t = Instant::now();
            std::hint::black_box(LayoutEngine::with_hyphenation(Arc::clone(reg)).layout(s.view(), fonts, &o).unwrap());
            fulls.push(ms(t));
        }
    }
    let (im, ip) = (median(&mut inc.clone()), p90(&mut inc));
    let fm = median(&mut fulls);
    report_json(
        name,
        "type1",
        &eng,
        im,
        &format!(
            ",\"p90_ms\":{ip:.2},\"full_ms\":{fm:.2},\"ratio\":{:.4},\"incremental\":{incremental},\"of\":21,\"relaid_max\":{relaid_max}",
            im / fm
        ),
    );
    // a 60-character insertion (wraps a line)
    exec(
        &mut s,
        &mut oracle,
        Command::Text(TextCmd::Type { text: Utf16::from("Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed ") }),
    );
    let t = Instant::now();
    std::hint::black_box(eng.relayout(s.view(), &[], fonts, &o).unwrap());
    report_json(name, "insert60", &eng, ms(t), "");
    // the next keystroke after it
    exec(&mut s, &mut oracle, Command::Text(TextCmd::Type { text: Utf16::from("b") }));
    let t = Instant::now();
    std::hint::black_box(eng.relayout(s.view(), &[], fonts, &o).unwrap());
    report_json(name, "type1-after-insert", &eng, ms(t), "");
    // Enter: a structure edit
    exec(&mut s, &mut oracle, Command::Text(TextCmd::Enter));
    let t = Instant::now();
    std::hint::black_box(eng.relayout(s.view(), &[], fonts, &o).unwrap());
    report_json(name, "enter", &eng, ms(t), "");
    // a keystroke after Enter (the base is fresh again)
    exec(&mut s, &mut oracle, Command::Text(TextCmd::Type { text: Utf16::from("c") }));
    let t = Instant::now();
    std::hint::black_box(eng.relayout(s.view(), &[], fonts, &o).unwrap());
    report_json(name, "type1-after-enter", &eng, ms(t), "");
    // memory: what the incremental state keeps besides the Layout (measured)
    let live0 = uo_alloc::thread_stats().live;
    eng.clear_incremental();
    let live1 = uo_alloc::thread_stats().live;
    std::hint::black_box(eng.relayout(s.view(), &[], fonts, &o).unwrap());
    let live2 = uo_alloc::thread_stats().live;
    let kept_all = live2 - live1;
    let mut plain = LayoutEngine::with_hyphenation(Arc::clone(reg));
    let live3 = uo_alloc::thread_stats().live;
    std::hint::black_box(plain.layout(s.view(), fonts, &o).unwrap());
    let live4 = uo_alloc::thread_stats().live;
    let layout_only = live4 - live3;
    drop(plain);
    // the same without the paragraph memo (level 2 alone): the memo's share
    let no_memo = IncrConfig { memo: MemoCaps { enabled: false, ..IncrConfig::default().memo }, ..IncrConfig::default() };
    let mut lvl2 = LayoutEngine::with_hyphenation(Arc::clone(reg)).with_incremental(no_memo);
    let live5 = uo_alloc::thread_stats().live;
    std::hint::black_box(lvl2.relayout(s.view(), &[], fonts, &o).unwrap());
    let live6 = uo_alloc::thread_stats().live;
    let kept_lvl2 = live6 - live5;
    report_json(
        name,
        "memory",
        &eng,
        0.0,
        &format!(
            ",\"measured_kept_bytes\":{kept_all},\"measured_kept_without_memo\":{kept_lvl2},\"measured_memo_bytes\":{},\"measured_layout_bytes\":{layout_only},\"incr_over_layout\":{:.2},\"live_before_clear\":{live0}",
            kept_all.saturating_sub(kept_lvl2),
            (kept_all - layout_only) as f64 / layout_only as f64
        ),
    );
}

fn main() {
    let repo = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"));
    let fonts = OpenFonts::at(&repo);
    let reg = hyph::hyph_registry();
    assert!(uo_alloc::is_counting(), "the counting allocator is not installed");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut gen_mode = false;
    for a in args {
        if a == "--gen" {
            gen_mode = true;
            continue;
        }
        if gen_mode {
            let n: usize = a.parse().unwrap();
            bench(&format!("gen{n}-numpages"), docgen::big_doc_pages(n, true), &fonts, &reg);
        } else {
            let name = std::path::Path::new(&a).file_stem().unwrap().to_string_lossy().into_owned();
            bench(&name, std::fs::read(&a).unwrap(), &fonts, &reg);
        }
    }
}
