//! G4 (cancellation inside the layout): `LayoutOptions::cancel` (a shared flag) and
//! `cancel_poll` (a polled function) end a layout with `LayoutError::Cancelled` from inside
//! `Budget::charge`; promptness, determinism, M1 (no leak) and a clean next layout.
//! `bench_cancel_check_cost` (opt-in, `UO_G4_BENCH=<runs>`) measures what the check costs.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

mod common;

use common::docgen::{big_doc, docx, para, sect};
use common::hyph::{hyph_opts as opts, hyph_registry};
use common::open_fonts::OpenFonts;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use uo_alloc::assert_no_leak;
use uo_docx_layout::Layout;
use uo_docx_layout::error::LayoutError;
use uo_docx_layout::fonts::FontProvider;
use uo_docx_layout::hyphenate::HyphenSessions;
use uo_docx_layout::options::LayoutOptions;

/// A document of every F7a path: sections (continuous with balanced columns, next page, even
/// page), keepNext/keepLines, widows, page breaks, border groups, shading, line numbers, lists,
/// contextual spacing, hidden and deleted marks, PAGE/NUMPAGES fields.
fn every_path() -> Vec<u8> {
    let mut b = String::new();
    for i in 0..40 {
        let ppr = match i % 10 {
            0 => r#"<w:pStyle w:val="H1"/>"#.to_owned(),
            1 => r#"<w:pBdr><w:top w:val="single" w:sz="4" w:space="1" w:color="auto"/><w:between w:val="single" w:sz="4" w:space="1" w:color="auto"/><w:bottom w:val="double" w:sz="4" w:space="1" w:color="auto"/></w:pBdr>"#.to_owned(),
            2 => r#"<w:pBdr><w:top w:val="single" w:sz="4" w:space="1" w:color="auto"/><w:between w:val="single" w:sz="4" w:space="1" w:color="auto"/><w:bottom w:val="double" w:sz="4" w:space="1" w:color="auto"/></w:pBdr><w:shd w:val="clear" w:fill="D9E2F3"/>"#.to_owned(),
            3 => r#"<w:keepLines/><w:pStyle w:val="C"/>"#.to_owned(),
            4 => r#"<w:pStyle w:val="C"/><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr>"#.to_owned(),
            5 => r#"<w:rPr><w:vanish/></w:rPr>"#.to_owned(),
            6 => r#"<w:rPr><w:del w:id="1" w:author="A" w:date="2026-01-01T00:00:00Z"/></w:rPr>"#.to_owned(),
            7 => r#"<w:pageBreakBefore/>"#.to_owned(),
            8 => r#"<w:widowControl/><w:spacing w:before="100" w:beforeAutospacing="1" w:after="100" w:afterAutospacing="1"/>"#.to_owned(),
            _ => String::new(),
        };
        b.push_str(&para(&ppr, 10 + (i * 13) % 90, i));
        if i == 9 {
            b.push_str(r#"<w:p><w:r><w:t>p. </w:t></w:r><w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple><w:fldSimple w:instr=" NUMPAGES "><w:r><w:t>1</w:t></w:r></w:fldSimple><w:r><w:br w:type="column"/><w:t>x</w:t></w:r></w:p>"#);
        }
        if i == 19 {
            b.push_str(&format!(
                r#"<w:p><w:pPr>{}</w:pPr></w:p>"#,
                sect(2, r#"<w:lnNumType w:countBy="2"/>"#, r#"<w:type w:val="continuous"/>"#)
            ));
        }
        if i == 29 {
            b.push_str(&format!(r#"<w:p><w:pPr>{}</w:pPr><w:r><w:t>fim</w:t></w:r></w:p>"#, sect(3, "", r#"<w:type w:val="evenPage"/>"#)));
        }
    }
    docx(
        &b,
        &sect(1, r#"<w:pgNumType w:start="3" w:fmt="lowerRoman"/>"#, r#"<w:type w:val="continuous"/>"#),
        "<w:defaultTabStop w:val=\"708\"/><w:autoHyphenation/>",
    )
}

fn layout_bytes(
    bytes: &[u8],
    fonts: &dyn FontProvider,
    reg: &Arc<uo_fonts::hyphen::HyphenationRegistry>,
    o: &LayoutOptions,
) -> Result<Layout, LayoutError> {
    let pkg = uo_opc::Package::open(bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default())
        .map_err(|_| LayoutError::Internal("test: package"))?;
    let doc =
        uo_docx::read::read_document(&pkg, &uo_docx::read::ReadOptions::default()).map_err(|_| LayoutError::Internal("test: read"))?;
    let ldoc = uo_docx_layout::derive::build(uo_docx::view::DocView::new(&doc), o)?;
    let mut hyph = HyphenSessions::new(Arc::clone(reg));
    uo_docx_layout::engine::layout_document(&ldoc, o, fonts, &mut hyph)
}

/// Reads and derives outside, then hands the derived document to `f`: the charges cover only the
/// layout, so the timed and the leak-checked regions must start after this.
fn prepared<R>(bytes: &[u8], o: &LayoutOptions, f: impl FnOnce(&uo_docx_layout::ldoc::LDoc<'_>) -> R) -> R {
    let pkg = uo_opc::Package::open(bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
    let doc = uo_docx::read::read_document(&pkg, &uo_docx::read::ReadOptions::default()).unwrap();
    let ldoc = uo_docx_layout::derive::build(uo_docx::view::DocView::new(&doc), o).unwrap();
    f(&ldoc)
}

fn layout_prepared(
    ldoc: &uo_docx_layout::ldoc::LDoc<'_>,
    fonts: &dyn FontProvider,
    reg: &Arc<uo_fonts::hyphen::HyphenationRegistry>,
    o: &LayoutOptions,
) -> Result<Layout, LayoutError> {
    let mut hyph = HyphenSessions::new(Arc::clone(reg));
    uo_docx_layout::engine::layout_document(ldoc, o, fonts, &mut hyph)
}

/// A sink that keeps the whole canonical stream.
#[derive(Default)]
struct VecSink(Vec<u8>);

impl uo_docx_layout::canon::CanonSink for VecSink {
    fn tag(&mut self, c: u8) {
        self.0.push(c);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    fn bool(&mut self, b: bool) {
        self.0.push(u8::from(b));
    }
    fn bytes(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
    }
    fn digest_of(&mut self, b: &[u8]) {
        self.0.extend_from_slice(&u64::try_from(b.len()).unwrap_or(u64::MAX).to_le_bytes());
        self.0.extend_from_slice(b);
    }
}

/// The P1 canonical stream of a layout (the whole item sequence, not a count).
fn canon_bytes(l: &Layout) -> Vec<u8> {
    let mut s = VecSink::default();
    uo_docx_layout::canon::canon_layout(l, &|_| None, &mut s);
    s.0
}

fn with_flag(flag: &Arc<AtomicBool>) -> LayoutOptions {
    LayoutOptions { cancel: Some(Arc::clone(flag)), ..opts() }
}

/// A fingerprint of the output: pages, items and lines per page.
fn fingerprint(l: &Layout) -> String {
    let mut s = format!("{} pages", l.pages.len());
    for p in &l.pages {
        s.push_str(&format!(" | {} items {} lines", p.page.list.items.len(), p.lines.len()));
    }
    s
}

static SERIAL: Mutex<()> = Mutex::new(());
static POLLS: AtomicUsize = AtomicUsize::new(0);
static STOP_AT: AtomicUsize = AtomicUsize::new(usize::MAX);

fn poll_stop_at() -> bool {
    POLLS.fetch_add(1, Ordering::Relaxed) + 1 >= STOP_AT.load(Ordering::Relaxed)
}

fn poll_opts() -> LayoutOptions {
    LayoutOptions { cancel_poll: Some(poll_stop_at), ..opts() }
}

fn run(
    bytes: &[u8],
    fonts: &dyn FontProvider,
    reg: &Arc<uo_fonts::hyphen::HyphenationRegistry>,
    o: &LayoutOptions,
) -> Result<Layout, LayoutError> {
    layout_bytes(bytes, fonts, reg, o)
}

#[test]
fn a_raised_flag_stops_before_any_work() {
    let (fonts, reg) = (OpenFonts::at(&common::repo()), hyph_registry());
    let flag = Arc::new(AtomicBool::new(true));
    let r = run(&every_path(), &fonts, &reg, &with_flag(&flag));
    assert_eq!(r.map(|_| ()), Err(LayoutError::Cancelled));
}

/// Raises the flag `delay` after `go`, then waits for `done` (it allocates nothing once started).
fn raiser(
    flag: Arc<AtomicBool>,
    go: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    raised_at: Arc<AtomicU64>,
    t0: Instant,
    delay: Duration,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while !go.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        std::thread::sleep(delay);
        raised_at.store(u64::try_from(t0.elapsed().as_nanos()).unwrap_or(u64::MAX), Ordering::Release);
        flag.store(true, Ordering::Release);
        while !done.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
    })
}

#[test]
fn raising_the_flag_mid_layout_returns_promptly() {
    let _g = SERIAL.lock().unwrap();
    let (fonts, reg) = (OpenFonts::at(&common::repo()), hyph_registry());
    // debug builds use a smaller document (the layout there is an order of magnitude slower)
    let bytes = big_doc(if cfg!(debug_assertions) { 150 } else { 650 });
    drop(run(&big_doc(5), &fonts, &reg, &opts()).map(|_| ())); // warm: the faces and hyphenation tables are the same
    for d_ms in [10u64, 40, 120] {
        let flag = Arc::new(AtomicBool::new(false));
        let (go, done, raised_at) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)), Arc::new(AtomicU64::new(0)));
        let o = with_flag(&flag);
        // read and derive outside: the clock and the raiser start right before the layout
        let (r, returned) = prepared(&bytes, &o, |ldoc| {
            let t0 = Instant::now();
            let h = raiser(Arc::clone(&flag), Arc::clone(&go), Arc::clone(&done), Arc::clone(&raised_at), t0, Duration::from_millis(d_ms));
            go.store(true, Ordering::Release);
            let r = layout_prepared(ldoc, &fonts, &reg, &o);
            let returned = u64::try_from(t0.elapsed().as_nanos()).unwrap();
            done.store(true, Ordering::Release);
            h.join().unwrap();
            (r, returned)
        });
        assert_eq!(
            r.map(|_| ()),
            Err(LayoutError::Cancelled),
            "D = {d_ms} ms: the document is too small for this machine (a release layout must take > 300 ms)"
        );
        let latency = Duration::from_nanos(returned.saturating_sub(raised_at.load(Ordering::Acquire)));
        eprintln!("G4 latency, raised at {d_ms} ms: {latency:?}");
        let bound = if cfg!(debug_assertions) { Duration::from_millis(2_000) } else { Duration::from_millis(250) };
        assert!(latency < bound, "latency {latency:?}");
    }
}

#[test]
fn poll_cancels_at_the_kth_poll_deterministically() {
    let _g = SERIAL.lock().unwrap();
    let (fonts, reg) = (OpenFonts::at(&common::repo()), hyph_registry());
    let bytes = big_doc(120);
    POLLS.store(0, Ordering::Relaxed);
    STOP_AT.store(usize::MAX, Ordering::Relaxed);
    run(&bytes, &fonts, &reg, &poll_opts()).unwrap();
    let n = POLLS.load(Ordering::Relaxed);
    eprintln!("G4: {n} polls in a 120-paragraph layout");
    assert!(n >= 2, "{n} polls");
    for k in [1, n / 2, n] {
        POLLS.store(0, Ordering::Relaxed);
        STOP_AT.store(k, Ordering::Relaxed);
        assert_eq!(run(&bytes, &fonts, &reg, &poll_opts()).map(|_| ()), Err(LayoutError::Cancelled), "k = {k}");
        assert_eq!(POLLS.load(Ordering::Relaxed), k, "the layout polls no more after the k-th answer");
    }
}

#[test]
fn m1_a_cancelled_layout_leaves_nothing() {
    let _g = SERIAL.lock().unwrap();
    let (fonts, reg) = (OpenFonts::at(&common::repo()), hyph_registry());
    let bytes = big_doc(120);
    POLLS.store(0, Ordering::Relaxed);
    STOP_AT.store(usize::MAX, Ordering::Relaxed);
    drop(run(&bytes, &fonts, &reg, &poll_opts()).unwrap()); // warm-up: faces and hyphenation tables cached
    let n = POLLS.load(Ordering::Relaxed);
    STOP_AT.store((n / 2).max(1), Ordering::Relaxed);
    POLLS.store(0, Ordering::Relaxed);
    // read and derive outside the leak check: only the layout is charged (and cancelled)
    let u = prepared(&bytes, &poll_opts(), |ldoc| {
        assert_no_leak(|| assert_eq!(layout_prepared(ldoc, &fonts, &reg, &poll_opts()).map(|_| ()), Err(LayoutError::Cancelled)))
    });
    eprintln!("M1/M3 cancelled by poll: {} B allocated, {} B peak, 0 B left", u.allocated, u.peak_above_start);
    // the flag variant: the raiser thread is created outside the closure and allocates nothing; the
    // flag goes up 5 ms after the layout starts, which the layout must outlast (checked below)
    let flag = Arc::new(AtomicBool::new(false));
    let o = with_flag(&flag);
    let (go, done) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let raised_at = Arc::new(AtomicU64::new(0));
    let h = raiser(Arc::clone(&flag), Arc::clone(&go), Arc::clone(&done), raised_at, Instant::now(), Duration::from_millis(5));
    let u = prepared(&bytes, &o, |ldoc| {
        assert_no_leak(|| {
            go.store(true, Ordering::Release);
            assert_eq!(layout_prepared(ldoc, &fonts, &reg, &o).map(|_| ()), Err(LayoutError::Cancelled));
        })
    });
    done.store(true, Ordering::Release);
    h.join().unwrap();
    // a raise before the layout started would allocate next to nothing; the poll run above shows the scale
    assert!(u.allocated > 100_000, "the flag was raised before the layout did any work ({} B allocated)", u.allocated);
    eprintln!("M1/M3 cancelled by flag: {} B allocated, {} B peak, 0 B left", u.allocated, u.peak_above_start);
}

#[test]
fn a_layout_after_a_cancelled_one_is_identical() {
    let _g = SERIAL.lock().unwrap();
    let bytes = big_doc(120);
    let (fonts, reg) = (OpenFonts::at(&common::repo()), hyph_registry());
    POLLS.store(0, Ordering::Relaxed);
    STOP_AT.store(3, Ordering::Relaxed);
    assert_eq!(run(&bytes, &fonts, &reg, &poll_opts()).map(|_| ()), Err(LayoutError::Cancelled));
    let after = run(&bytes, &fonts, &reg, &opts()).unwrap();
    let (fresh_fonts, fresh_reg) = (OpenFonts::at(&common::repo()), hyph_registry());
    let fresh = run(&bytes, &fresh_fonts, &fresh_reg, &opts()).unwrap();
    assert_eq!(fingerprint(&after), fingerprint(&fresh));
    // the whole item sequence, not only the counts
    assert!(canon_bytes(&after) == canon_bytes(&fresh), "the canonical streams differ");
}

static BENCH_POLLS: AtomicUsize = AtomicUsize::new(0);
fn counting_poll() -> bool {
    BENCH_POLLS.fetch_add(1, Ordering::Relaxed);
    false
}

/// Opt-in: `UO_G4_BENCH=<runs>` (and `UO_G4_DOC=<docx>`): the cost of the cancel check.
#[test]
fn bench_cancel_check_cost() {
    let Some(runs) = std::env::var("UO_G4_BENCH").ok().and_then(|v| v.parse::<usize>().ok()) else { return };
    let _g = SERIAL.lock().unwrap();
    let bytes = std::env::var("UO_G4_DOC").ok().map_or_else(|| big_doc(650), |p| std::fs::read(p).unwrap());
    let (fonts, reg) = (OpenFonts::at(&common::repo()), hyph_registry());
    let flag = Arc::new(AtomicBool::new(false));
    let modes: [(&str, LayoutOptions); 3] =
        [("A none", opts()), ("B flag", with_flag(&flag)), ("C poll", LayoutOptions { cancel_poll: Some(counting_poll), ..opts() })];
    for (_, o) in &modes {
        drop(run(&bytes, &fonts, &reg, o).unwrap());
    }
    let mut ms: [Vec<f64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    BENCH_POLLS.store(0, Ordering::Relaxed);
    for _ in 0..runs {
        for (i, (_, o)) in modes.iter().enumerate() {
            let t = Instant::now();
            drop(run(&bytes, &fonts, &reg, o).unwrap());
            ms[i].push(t.elapsed().as_secs_f64() * 1000.0);
        }
    }
    let polls = BENCH_POLLS.load(Ordering::Relaxed) / runs;
    let stat = |v: &Vec<f64>| {
        let mut s = v.clone();
        s.sort_by(f64::total_cmp);
        (s[0], s[s.len() / 2])
    };
    let (a, b, c) = (stat(&ms[0]), stat(&ms[1]), stat(&ms[2]));
    for (name, s) in [("A none", a), ("B flag", b), ("C poll", c)] {
        eprintln!("G4 bench {name}: min {:.1} ms, median {:.1} ms", s.0, s.1);
    }
    eprintln!(
        "G4 bench B/A: min {:+.2}% median {:+.2}%; C/A: min {:+.2}% median {:+.2}%; {polls} polls per layout, {:.2} ms per poll interval",
        (b.0 / a.0 - 1.0) * 100.0,
        (b.1 / a.1 - 1.0) * 100.0,
        (c.0 / a.0 - 1.0) * 100.0,
        (c.1 / a.1 - 1.0) * 100.0,
        a.0 / polls.max(1) as f64
    );
}
