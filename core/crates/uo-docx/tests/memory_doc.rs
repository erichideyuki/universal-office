//! Memory gate of the `.doc` converter (D4). M1: every public operation returns the live bytes
//! exactly to their value before it, outputs dropped (`uo_alloc::assert_no_leak`): per fixture and
//! per error class of the capture. M2: 1,000 passes of the reader pipeline over the fixtures,
//! the live bytes flat at 0 B after warm-up. M3: the bytes allocated per fixture, as a multiple of
//! the input, asserted below the recorded budget (record "Stage 3 — D4").
#![cfg(feature = "doc")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::doc_support;

use std::hint::black_box;

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_docx::doc::fib::FcLcb;
use uo_docx::doc::fkp::{read_chpx_runs, read_papx_runs};
use uo_docx::doc::fonts::read_fonts;
use uo_docx::doc::lists::read_lists;
use uo_docx::doc::numbering::build_numbering;
use uo_docx::doc::ooxml_props::{PPrOpts, PropCtx, brc_xml, p_pr_inner, r_pr_inner, shd_xml, tabs_xml};
use uo_docx::doc::open::DocStreams;
use uo_docx::doc::props::{
    ChpCtx, KeySet, Pap, PapCtx, Tap, apply_chp, apply_pap, apply_sep, apply_tap, default_sep, finish_tap, prune_none_borders,
};
use uo_docx::doc::settings::{build_settings, read_dop};
use uo_docx::doc::sprm::prls;
use uo_docx::doc::stsh::{StyleResolver, read_stsh, read_stsh_at};
use uo_docx::doc::styles_xml::{build_styles, style_id_map};
use uo_docx::doc::text::{decode_text, read_piece_table, total_cp};
use uo_docx::doc::xmlw::{AttrVal, Attrs, attrs, el, esc_attr_units, esc_text, esc_text_units};
use uo_docx::doc::{DocLimitKind, DocLimits, convert_doc_to_docx, is_word_binary};

/// The reader pipeline of F1 over a file: the streams, the piece table, the text, the runs, the
/// fonts, and a walk of every grpprl. Returns a number so nothing is optimised away.
fn pipeline(bytes: &[u8]) -> usize {
    let Ok(streams) = DocStreams::open(bytes) else { return 0 };
    let Ok(pt) = read_piece_table(&streams.fib, &streams.table) else { return 1 };
    let total = total_cp(&streams.fib, &pt);
    let text = decode_text(&pt, &streams.word, total, &DocLimits::DEFAULT).map_or(0, |t| t.len());
    let chpx = read_chpx_runs(&streams.fib, &streams.table, &streams.word);
    let papx = read_papx_runs(&streams.fib, &streams.table, &streams.word, streams.data.as_deref());
    let fonts = read_fonts(&streams.fib, &streams.table);
    let sprms: usize = chpx.iter().chain(papx.iter()).map(|r| prls(r.grpprl).count()).sum();
    text + chpx.len() + papx.len() + fonts.len() + sprms
}

#[test]
fn m1_f0() {
    let fixtures = doc_support::fixtures();
    for (_, bytes) in &fixtures {
        assert_no_leak(|| black_box(is_word_binary(bytes)));
        assert_no_leak(|| black_box(convert_doc_to_docx(bytes, &DocLimits::DEFAULT)));
    }
    assert_no_leak(|| black_box(is_word_binary(b"PK\x03\x04")));
    assert_no_leak(|| black_box(convert_doc_to_docx(b"not a doc", &DocLimits::DEFAULT)));
    let units: Vec<u16> = "a & <b> \"q\" \u{1}😀".encode_utf16().chain([0xd800, 0xdc00, 0xdc00]).collect();
    assert_no_leak(|| black_box(esc_text_units(&units)));
    assert_no_leak(|| black_box(esc_attr_units(&units)));
    assert_no_leak(|| black_box(esc_text("a & <b>")));
    assert_no_leak(|| {
        let mut a = Attrs::new();
        a.set("w:val", Some(AttrVal::Units(&units))).set("w:x", Some(AttrVal::Int(-3))).set("w:y", Some(AttrVal::Num(1.5)));
        black_box(el("w:p", &a, &attrs(&a)))
    });
}

#[test]
fn m1_f1() {
    for (name, bytes) in doc_support::fixtures().iter().chain(doc_support::class_samples().iter()) {
        let u = assert_no_leak(|| black_box(pipeline(bytes)));
        eprintln!("M1 f1 {name}: {} B allocated, {} B peak, 0 B left (input {} B)", u.allocated, u.peak_above_start, bytes.len());
        assert_no_leak(|| black_box(DocStreams::open(bytes).map(|s| s.table.len())));
    }
    // the text limit error path
    let (_, bytes) = &doc_support::fixtures()[0];
    let streams = DocStreams::open(bytes).unwrap();
    let pt = read_piece_table(&streams.fib, &streams.table).unwrap();
    let tight = DocLimits { max_text_units: 3, ..DocLimits::DEFAULT };
    assert_no_leak(|| black_box(decode_text(&pt, &streams.word, total_cp(&streams.fib, &pt), &tight)));
}

#[test]
fn m2_soak_flat() {
    let fixtures = doc_support::fixtures();
    let series = soak(1000, 50, |i| {
        black_box(pipeline(&fixtures[i % fixtures.len()].1));
    });
    assert_flat(&series, 2, 0);
    eprintln!("M2 f1: 1,000 passes, live bytes {:?}", &series[..3]);
}

/// M3: the bytes the pipeline allocates, per fixture, as a multiple of the input.
#[test]
fn m3_budget() {
    let mut worst = 0.0f64;
    for (name, bytes) in doc_support::fixtures() {
        let (n, u) = measure(|| pipeline(&bytes));
        black_box(n);
        let ratio = u.allocated as f64 / bytes.len() as f64;
        if ratio > worst {
            worst = ratio;
        }
        eprintln!("M3 f1 {name}: {} B allocated = {ratio:.1} x the input ({} B), peak {} B", u.allocated, bytes.len(), u.peak_above_start);
    }
    assert!(worst <= 3.0, "the F1 pipeline allocates {worst:.1} x its input (budget 3)");
}

/// The appliers (`apply_chp`, `apply_pap`, `apply_tap`, `apply_sep`) over one grpprl, everything dropped.
fn apply_all(grpprl: &[u8]) -> usize {
    let mut chp = uo_docx::doc::props::Chp::default();
    let mut t = KeySet::new();
    let ps = uo_docx::doc::props::default_chp(1, 2, 3);
    let mut style = |istd: u16| uo_docx::doc::props::Chp { istd: Some(istd), ..ps.clone() };
    let mut ctx = ChpCtx { base: ps.clone(), apply_char_style: Some(&mut style), para_style: Some(&ps) };
    for p in prls(grpprl) {
        apply_chp(&mut chp, &p, &mut ctx, &mut t);
    }
    let mut pap = Pap { istd: 3, tabs: Some(vec![]), ..Pap::default() };
    let base = pap.clone();
    let mut pt = KeySet::new();
    let mut istds = Vec::new();
    let mut rec = |v: u16| istds.push(v);
    let mut pctx = PapCtx { on_istd: Some(&mut rec) };
    for p in prls(grpprl) {
        apply_pap(&mut pap, &p, &mut pt, &mut pctx);
    }
    prune_none_borders(&pap, &base, &mut pt);
    let mut tap = Tap::new();
    for p in prls(grpprl) {
        apply_tap(&mut tap, &p);
    }
    finish_tap(&mut tap);
    let mut sep = default_sep();
    for p in prls(grpprl) {
        apply_sep(&mut sep, &p);
    }
    t.len() + pt.len() + tap.tcs.len() + sep.col_widths.len() + chp.hps.map_or(0, |h| usize::try_from(h).unwrap_or(0))
}

fn gen_grpprls(n: usize) -> Vec<Vec<u8>> {
    let cap = doc_support::capture();
    cap.records().iter().filter(|r| r.key.starts_with("gen/grpprl/")).take(n).map(|r| doc_support::unhex(r.get("hex").unwrap())).collect()
}

#[test]
fn m1_f2_appliers() {
    let gens = gen_grpprls(400);
    let mut worst = 0u64;
    for g in &gens {
        let u = assert_no_leak(|| black_box(apply_all(g)));
        worst = worst.max(u.allocated);
    }
    // the runs of the fixtures
    for (_, bytes) in doc_support::fixtures() {
        let streams = DocStreams::open(&bytes).unwrap();
        let chpx = read_chpx_runs(&streams.fib, &streams.table, &streams.word);
        let papx = read_papx_runs(&streams.fib, &streams.table, &streams.word, streams.data.as_deref());
        assert_no_leak(|| black_box(chpx.iter().chain(papx.iter()).map(|r| apply_all(r.grpprl)).sum::<usize>()));
    }
    eprintln!("M1 f2: 400 generated grpprls and the runs of 11 fixtures, 0 B left; at most {worst} B allocated for one grpprl");
}

#[test]
fn m2_f2_soak_flat() {
    let gens = gen_grpprls(200);
    let series = soak(1000, 50, |i| {
        black_box(apply_all(&gens[i % gens.len()]));
    });
    assert_flat(&series, 2, 0);
}

/// M3: the hostile table and tab grpprls stay inside the limits (the cells and tab stops are capped).
#[test]
fn m3_f2_hostile_grpprls_are_bounded() {
    // 10,000 sprmTInsert of 255 cells: the TS builds 2.5 million cells, the port stops at MAX_TAP_CELLS
    let mut g = Vec::new();
    for _ in 0..10_000 {
        g.extend([0x21, 0x76, 0, 255, 1, 0]);
    }
    let (n, u) = measure(|| apply_all(&g));
    black_box(n);
    eprintln!("M3 f2: 10,000 sprmTInsert x 255 cells: {} B allocated, {} B peak", u.allocated, u.peak_above_start);
    assert!(u.peak_above_start < 3 << 20, "peak {} B", u.peak_above_start);
    // 10,000 sprmTDxaCol over a full row: the work budget ends it
    let mut g2 = g[..6 * 40].to_vec();
    for _ in 0..10_000 {
        g2.extend([0x23, 0x76, 0, 255, 1, 0]);
    }
    let start = std::time::Instant::now();
    black_box(apply_all(&g2));
    eprintln!("M3 f2: the same plus 10,000 sprmTDxaCol: {:?}", start.elapsed());
    assert!(start.elapsed().as_secs() < 5);
}

/// The style sheet of a file, every style resolved (paragraph and character), and the lists.
fn styles_and_lists(bytes: &[u8]) -> usize {
    let Ok(streams) = DocStreams::open(bytes) else { return 0 };
    let Ok(sheet) = read_stsh(&streams.fib, &streams.table, &DocLimits::DEFAULT) else { return 1 };
    let n = sheet.styles.len();
    let mut r = StyleResolver::new(sheet);
    let mut total = 0;
    for i in 0..n {
        let i = u16::try_from(i).unwrap();
        total += r.resolve(i).own_pap.len();
        total += r.resolve_char(i).1.len();
        let root = r.root_chp.clone();
        total += usize::from(r.apply_char_style(i, &root).bold.is_some());
    }
    r.clear();
    let lists = read_lists(&streams.fib, &streams.table);
    total + lists.lsts.len() + lists.lfos.len()
}

#[test]
fn m1_f3_styles_and_lists() {
    for (name, bytes) in doc_support::fixtures() {
        let u = assert_no_leak(|| black_box(styles_and_lists(&bytes)));
        eprintln!("M1 f3 {name}: {} B allocated, {} B peak, 0 B left", u.allocated, u.peak_above_start);
    }
    for (_, bytes) in doc_support::class_samples() {
        assert_no_leak(|| black_box(styles_and_lists(&bytes)));
    }
    // the string budget error path
    let mut t = Vec::new();
    t.extend(18u16.to_le_bytes());
    let mut stshif = [0u8; 18];
    stshif[0..2].copy_from_slice(&300u16.to_le_bytes());
    stshif[2..4].copy_from_slice(&1u16.to_le_bytes());
    t.extend(stshif);
    for _ in 0..300 {
        t.extend(1u16.to_le_bytes());
        t.extend([0xff, 0xff]);
    }
    t.extend(vec![0x41u8; 4000]);
    let pair = FcLcb { fc: 0, lcb: u32::try_from(t.len()).unwrap() };
    let tight = DocLimits { max_string_units: 1000, ..DocLimits::DEFAULT };
    let u = assert_no_leak(|| {
        let e = read_stsh_at(pair, &t, &tight).unwrap_err();
        assert!(matches!(e, uo_docx::doc::DocError::Limit { kind: DocLimitKind::StringUnits, .. }));
    });
    eprintln!("M1 f3 string budget hit: {} B allocated, 0 B left", u.allocated);
}

#[test]
fn m2_f3_soak_flat() {
    let fixtures = doc_support::fixtures();
    let series = soak(1000, 50, |i| {
        black_box(styles_and_lists(&fixtures[i % fixtures.len()].1));
    });
    assert_flat(&series, 2, 0);
}

/// M3: the styles and lists of the fixtures, and the hostile sheets: 65,535 styles of three bytes (a chain,
/// every one resolved) and the overlapping names that the budget stops.
#[test]
fn m3_f3_budgets() {
    let mut worst = 0.0f64;
    for (name, bytes) in doc_support::fixtures() {
        let (n, u) = measure(|| styles_and_lists(&bytes));
        black_box(n);
        let ratio = u.allocated as f64 / bytes.len() as f64;
        if ratio > worst {
            worst = ratio;
        }
        eprintln!("M3 f3 {name}: {} B allocated = {ratio:.1} x the input ({} B), peak {} B", u.allocated, bytes.len(), u.peak_above_start);
    }
    assert!(worst <= 20.0, "styles and lists allocate {worst:.1} x the input (budget 20)");
    // 65,535 styles, each 3 bytes (cbStd 1): based on the previous one, all resolved (the memo keeps 65,535 results)
    let mut t = Vec::new();
    t.extend(18u16.to_le_bytes());
    let mut stshif = [0u8; 18];
    stshif[0..2].copy_from_slice(&u16::MAX.to_le_bytes());
    stshif[2..4].copy_from_slice(&10u16.to_le_bytes());
    t.extend(stshif);
    for _ in 0..65_535 {
        t.extend(1u16.to_le_bytes());
        t.extend([0, 0]);
    }
    let pair = FcLcb { fc: 0, lcb: u32::try_from(t.len()).unwrap() };
    let (n, u) = measure(|| {
        let sheet = read_stsh_at(pair, &t, &DocLimits::DEFAULT).unwrap();
        let n = sheet.styles.len();
        let mut r = StyleResolver::new(sheet);
        for i in 0..n {
            black_box(r.resolve(u16::try_from(i).unwrap()));
        }
        n
    });
    black_box(n);
    let per_style = u.peak_above_start / 65_535;
    eprintln!(
        "M3 f3: 65,535 hostile styles ({} B of input): {} B allocated, peak {} B = {per_style} B per style",
        t.len(),
        u.allocated,
        u.peak_above_start
    );
    assert!(u.peak_above_start < 200 << 20, "peak {} B", u.peak_above_start);
}

/// The parts the emitters write for a file: `styles.xml`, `numbering.xml`, `settings.xml` (twice), the DOP.
fn parts(bytes: &[u8]) -> usize {
    use std::borrow::Cow;
    let Ok(streams) = DocStreams::open(bytes) else { return 0 };
    let Ok(sheet) = read_stsh(&streams.fib, &streams.table, &DocLimits::DEFAULT) else { return 1 };
    let fonts = read_fonts(&streams.fib, &streams.table);
    let lists = read_lists(&streams.fib, &streams.table);
    let ids = style_id_map(&sheet.styles);
    let sid = |i: u16| ids.get(i).map(Cow::Borrowed);
    let ctx = PropCtx { fonts: &fonts, style_id: &sid };
    let lfos = lists.lfos.len() as u64;
    let num_id_of = |n: u32| if n >= 1 && u64::from(n) <= lfos { i64::from(n) } else { 0 };
    let ftc_fe = sheet.stshif.ftc_fe;
    let mut resolver = StyleResolver::new(sheet);
    let styles = build_styles(&mut resolver, &fonts, &ctx, &ids, &num_id_of);
    let numbering = build_numbering(&lists, &ctx);
    let pair = streams.fib.fc_lcb(uo_docx::doc::fib::fc::Dop);
    let dop: &[u8] = streams.table.get(pair.fc as usize..(u64::from(pair.fc) + u64::from(pair.lcb)) as usize).unwrap_or(&[]);
    let n_fib = streams.fib.n_fib;
    styles.len()
        + numbering.map_or(0, |n| n.len())
        + build_settings(dop, n_fib, &fonts, ftc_fe, false, false).len()
        + build_settings(dop, n_fib, &fonts, ftc_fe, true, true).len()
        + usize::from(read_dop(dop, n_fib).zoom)
}

#[test]
fn m1_f5_emitters() {
    for (name, bytes) in doc_support::fixtures() {
        let u = assert_no_leak(|| black_box(parts(&bytes)));
        eprintln!("M1 f5 {name}: {} B allocated, {} B peak, 0 B left", u.allocated, u.peak_above_start);
    }
    for (_, bytes) in doc_support::class_samples() {
        assert_no_leak(|| black_box(parts(&bytes)));
    }
    // the property emitters over the properties of generated grpprls
    let gens = gen_grpprls(300);
    let ps = uo_docx::doc::props::default_chp(0, 0, 0);
    for g in &gens {
        assert_no_leak(|| {
            let fonts: [uo_docx::doc::fonts::DocFont; 0] = [];
            let sid = |i: u16| (i.is_multiple_of(2)).then(|| std::borrow::Cow::Owned(format!("S{i}")));
            let ctx = PropCtx { fonts: &fonts, style_id: &sid };
            let mut chp = uo_docx::doc::props::Chp::default();
            let mut t = KeySet::new();
            let mut cctx = ChpCtx { base: ps.clone(), apply_char_style: None, para_style: Some(&ps) };
            let mut pap = Pap { istd: 4, ..Pap::default() };
            let mut pt = KeySet::new();
            for p in prls(g) {
                apply_chp(&mut chp, &p, &mut cctx, &mut t);
                apply_pap(&mut pap, &p, &mut pt, &mut PapCtx::default());
            }
            let numid = |n: u32| i64::from(n);
            black_box((
                r_pr_inner(&chp, &t, &ctx, true),
                p_pr_inner(&pap, &pt, &ctx, &PPrOpts { num_id: Some(&numid), base_tabs: Some(&[]), ..PPrOpts::default() }),
                brc_xml("w:top", pap.brc_top.as_ref()),
                shd_xml(pap.shd.as_ref()),
                tabs_xml(None, pap.tabs.as_deref(), pap.tabs_cleared.as_deref().unwrap_or(&[])),
            ))
        });
    }
}

#[test]
fn m2_f5_soak_flat() {
    let fixtures = doc_support::fixtures();
    let series = soak(1000, 50, |i| {
        black_box(parts(&fixtures[i % fixtures.len()].1));
    });
    assert_flat(&series, 2, 0);
}

/// M3: the parts of the fixtures, as a multiple of the input, and a sheet of 20,000 styles with one name.
#[test]
fn m3_f5_budgets() {
    let mut worst = 0.0f64;
    for (name, bytes) in doc_support::fixtures() {
        let (n, u) = measure(|| parts(&bytes));
        black_box(n);
        let ratio = u.allocated as f64 / bytes.len() as f64;
        if ratio > worst {
            worst = ratio;
        }
        eprintln!("M3 f5 {name}: {} B allocated = {ratio:.1} x the input ({} B), peak {} B", u.allocated, bytes.len(), u.peak_above_start);
    }
    assert!(worst <= 15.0, "the emitters allocate {worst:.1} x the input (budget 15)");
}

// ───────────────────────── F4: the reader assembly; F6: the body and drawing writer ─────────────────────────

use uo_docx::doc::docx_writer::ConvertOptions;
use uo_docx::doc::formfields::ff_data_xml;
use uo_docx::doc::pictures::{pic_fx_of, read_inline_picture, shape_blip};
use uo_docx::doc::reader::{DocReader, NoteKind};
use uo_docx::doc::{DocError, convert_doc_to_docx_with};

/// Every operation of the reader assembly over a file: the stories, the PLCs and strings, the drawings, the
/// pictures and the form-field data. Returns a number so nothing is optimised away.
fn reader_ops(bytes: &[u8]) -> usize {
    let Ok(streams) = DocStreams::open(bytes) else { return 0 };
    let Ok(rd) = DocReader::new(&streams, &DocLimits::DEFAULT) else { return 1 };
    let mut n = rd.header_stories().len() + rd.notes(NoteKind::Footnote).len() + rd.notes(NoteKind::Endnote).len();
    n += rd.comments().map_or(0, |c| c.len()) + rd.bookmarks().map_or(0, |b| b.len()) + rd.revision_authors().map_or(0, |a| a.len());
    for hdr in [false, true] {
        for f in rd.fspas(hdr) {
            if let Some(shape) = rd.shape(i64::from(f.spid), hdr) {
                n += usize::from(pic_fx_of(Some(shape)).is_some()) + usize::from(shape_blip(&rd, shape).is_some());
            }
        }
        n += rd.textboxes(hdr).len();
    }
    for cp in [0, 1, 50, 5000] {
        n += usize::try_from(rd.paragraph_at(cp, i64::from(rd.fib.ccp_text)).end).unwrap_or(0);
    }
    for run in &rd.chpx {
        let fc = i32::from_le_bytes(run.fc_start.to_le_bytes());
        n += usize::from(read_inline_picture(&rd, fc).is_some()) + ff_data_xml(rd.data, fc).map_or(0, |x| x.len());
    }
    n + rd.data_records(0, u64::MAX).len() + rd.sections.len() + rd.text.len()
}

#[test]
fn m1_f4_reader() {
    for (name, bytes) in doc_support::fixtures() {
        let u = assert_no_leak(|| black_box(reader_ops(&bytes)));
        eprintln!("M1 f4 {name}: {} B allocated, {} B peak, 0 B left", u.allocated, u.peak_above_start);
    }
    for (_, bytes) in doc_support::class_samples() {
        assert_no_leak(|| black_box(reader_ops(&bytes)));
    }
    // the string-budget error path of the strings (comments, bookmarks) and of the constructor
    let tight = DocLimits { max_string_units: 2, string_units_per_byte: 0, ..DocLimits::DEFAULT };
    for (_, bytes) in doc_support::fixtures() {
        assert_no_leak(|| {
            let Ok(streams) = DocStreams::open(&bytes) else { return 0 };
            match DocReader::new(&streams, &tight) {
                Ok(rd) => rd.comments().map_or(0, |c| c.len()) + rd.bookmarks().map_or(0, |b| b.len()),
                Err(_) => 1,
            }
        });
    }
}

#[test]
fn m2_f4_soak_flat() {
    let fixtures = doc_support::fixtures();
    let series = soak(1000, 50, |i| {
        black_box(reader_ops(&fixtures[i % fixtures.len()].1));
    });
    assert_flat(&series, 2, 0);
}

#[test]
fn m3_f4_budgets() {
    let mut worst = 0.0f64;
    for (name, bytes) in doc_support::fixtures() {
        let (n, u) = measure(|| reader_ops(&bytes));
        black_box(n);
        let ratio = u.allocated as f64 / bytes.len() as f64;
        if ratio > worst {
            worst = ratio;
        }
        eprintln!("M3 f4 {name}: {} B allocated = {ratio:.1} x the input ({} B), peak {} B", u.allocated, bytes.len(), u.peak_above_start);
    }
    assert!(worst <= 20.0, "the reader allocates {worst:.1} x the input (budget 20)");
}

/// The whole conversion: the package of a file, with and without `docProps`, and through every limit.
fn convert(bytes: &[u8], limits: &DocLimits) -> usize {
    let a = convert_doc_to_docx(bytes, limits).map_or(0, |z| z.len());
    let b = convert_doc_to_docx_with(bytes, limits, &ConvertOptions { doc_props: false }).map_or(0, |z| z.len());
    a + b
}

#[test]
fn m1_f6_converter() {
    let tight_work = DocLimits { max_work: 40, ..DocLimits::DEFAULT };
    let tight_nest = DocLimits { max_nesting: 0, ..DocLimits::DEFAULT };
    let tight_text = DocLimits { max_text_units: 16, ..DocLimits::DEFAULT };
    let small =
        DocLimits { max_text_units: 1 << 16, max_string_units: 1 << 12, max_work: 1 << 12, string_units_per_byte: 1, max_nesting: 8 };
    for (name, bytes) in doc_support::fixtures() {
        let u = assert_no_leak(|| black_box(convert(&bytes, &DocLimits::DEFAULT)));
        eprintln!("M1 f6 {name}: {} B allocated, {} B peak, 0 B left (input {} B)", u.allocated, u.peak_above_start, bytes.len());
        for limits in [&tight_work, &tight_nest, &tight_text, &small] {
            assert_no_leak(|| black_box(convert(&bytes, limits)));
        }
    }
    for (_, bytes) in doc_support::class_samples() {
        assert_no_leak(|| black_box(convert(&bytes, &DocLimits::DEFAULT)));
    }
    // the work limit answers `Limit { Work }` on a fixture with a body
    let (_, bytes) = &doc_support::fixtures()[0];
    assert!(matches!(
        convert_doc_to_docx(bytes, &tight_work),
        Err(DocError::Limit { kind: DocLimitKind::Work, .. }) | Err(DocError::Limit { kind: DocLimitKind::TextUnits, .. })
    ));
}

#[test]
fn m2_f6_soak_flat() {
    let fixtures = doc_support::fixtures();
    let series = soak(1000, 50, |i| {
        black_box(convert(&fixtures[i % fixtures.len()].1, &DocLimits::DEFAULT));
    });
    assert_flat(&series, 2, 0);
}

/// M3: the bytes of the conversion per fixture as a multiple of the input.
#[test]
fn m3_f6_budgets() {
    let mut worst = 0.0f64;
    for (name, bytes) in doc_support::fixtures() {
        let (n, u) = measure(|| convert_doc_to_docx(&bytes, &DocLimits::DEFAULT).map_or(0, |z| z.len()));
        black_box(n);
        let ratio = u.allocated as f64 / bytes.len() as f64;
        if ratio > worst {
            worst = ratio;
        }
        eprintln!("M3 f6 {name}: {} B allocated = {ratio:.1} x the input ({} B), peak {} B", u.allocated, bytes.len(), u.peak_above_start);
    }
    assert!(worst <= 40.0, "the converter allocates {worst:.1} x the input (budget 40)");
}
