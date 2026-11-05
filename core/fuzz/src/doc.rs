//! The `doc_convert` target of the `.doc` converter (uo-docx D4, record `core/docs/modules/uo-docx.md`,
//! "D4 — .doc (frontier briefs)" and "Stage 3 — D4"): a whole `.doc` file through the converter and
//! through every structure reader that exists (the streams, the piece table and the text, the FKP
//! runs, the fonts, each grpprl applied to character, paragraph, table and section properties, the
//! style sheet and its resolver, the lists, the part emitters).
//!
//! Input: a selector byte (bit 0: small `DocLimits`), then the file.

use std::hint::black_box;
use std::path::Path;

use uo_docx::doc::fkp::{read_chpx_runs, read_papx_runs};
use uo_docx::doc::fonts::read_fonts;
use uo_docx::doc::lists::read_lists;
use uo_docx::doc::numbering::build_numbering;
use uo_docx::doc::ooxml_props::{PPrOpts, PropCtx, p_pr_inner, r_pr_inner};
use uo_docx::doc::open::DocStreams;
use uo_docx::doc::props::{
    Chp, ChpCtx, KeySet, Pap, PapCtx, Tap, apply_chp, apply_pap, apply_sep, apply_tap, default_chp, default_sep, finish_tap,
    prune_none_borders,
};
use uo_docx::doc::settings::{build_settings, read_dop};
use uo_docx::doc::sprm::prls;
use uo_docx::doc::stsh::{StyleResolver, read_stsh};
use uo_docx::doc::styles_xml::{build_styles, style_id_map};
use uo_docx::doc::text::{decode_text, read_piece_table, total_cp};
use uo_docx::doc::{DocError, DocLimitKind, DocLimits, convert_doc_to_docx};

/// Small limits: every limit is reached by small inputs.
pub const DOC_LIMITS: DocLimits =
    DocLimits { max_text_units: 1 << 16, max_string_units: 1 << 12, max_work: 1 << 12, string_units_per_byte: 1 };

pub fn doc_convert(data: &[u8]) {
    let (sel, file) = match data.split_first() {
        Some((&s, rest)) => (s, rest),
        None => (0, &[][..]),
    };
    let limits = if sel & 1 == 1 { DOC_LIMITS } else { DocLimits::DEFAULT };
    match convert_doc_to_docx(file, &limits) {
        Ok(docx) => {
            black_box(docx.len());
        }
        Err(DocError::Limit { max, .. }) => assert!(max > 0 || limits.max_text_units == 0),
        Err(e) => {
            black_box(e.class());
        }
    }
    let Ok(streams) = DocStreams::open(file) else { return };
    let Ok(pt) = read_piece_table(&streams.fib, &streams.table) else { return };
    match decode_text(&pt, &streams.word, total_cp(&streams.fib, &pt), &limits) {
        Ok(t) => assert!(t.len() as u64 <= limits.max_text_units),
        Err(DocError::Limit { kind: DocLimitKind::TextUnits, .. }) => {}
        Err(e) => panic!("{e}"),
    }
    let chpx = read_chpx_runs(&streams.fib, &streams.table, &streams.word);
    let papx = read_papx_runs(&streams.fib, &streams.table, &streams.word, streams.data.as_deref());
    let ps = default_chp(1, 2, 3);
    for r in chpx.iter().chain(papx.iter()) {
        black_box(prls(r.grpprl).count());
        let mut chp = Chp::default();
        let mut t = KeySet::new();
        let mut style = |istd: u16| Chp { istd: Some(istd), ..ps.clone() };
        let mut ctx = ChpCtx { base: ps.clone(), apply_char_style: Some(&mut style), para_style: Some(&ps) };
        let mut pap = Pap { istd: r.istd, ..Pap::default() };
        let base = pap.clone();
        let mut pt = KeySet::new();
        let mut tap = Tap::new();
        let mut sep = default_sep();
        for p in prls(r.grpprl) {
            apply_chp(&mut chp, &p, &mut ctx, &mut t);
            apply_pap(&mut pap, &p, &mut pt, &mut PapCtx::default());
            apply_tap(&mut tap, &p);
            apply_sep(&mut sep, &p);
        }
        prune_none_borders(&pap, &base, &mut pt);
        finish_tap(&mut tap);
        assert!(tap.tcs.len() <= 1 << 12 && pap.tabs.as_ref().is_none_or(|t| t.len() <= 256));
        let fonts = read_fonts(&streams.fib, &streams.table);
        let none = |_: u16| None;
        let ctx = PropCtx { fonts: &fonts, style_id: &none };
        black_box(r_pr_inner(&chp, &t, &ctx, true));
        black_box(p_pr_inner(&pap, &pt, &ctx, &PPrOpts { base_tabs: Some(&[]), ..PPrOpts::default() }));
    }
    black_box(read_fonts(&streams.fib, &streams.table));
    match read_stsh(&streams.fib, &streams.table, &limits) {
        Ok(sheet) => {
            let n = sheet.styles.len().min(200);
            let fonts = read_fonts(&streams.fib, &streams.table);
            let lists = read_lists(&streams.fib, &streams.table);
            let ids = style_id_map(&sheet.styles);
            let sid = |i: u16| ids.get(i).map(std::borrow::Cow::Borrowed);
            let ctx = PropCtx { fonts: &fonts, style_id: &sid };
            let ftc_fe = sheet.stshif.ftc_fe;
            let mut r = StyleResolver::new(sheet.clone());
            let xml = build_styles(&mut r, &fonts, &ctx, &ids, &|n| i64::from(n));
            assert!(xml.starts_with("<?xml"));
            black_box(build_numbering(&lists, &ctx));
            let pair = streams.fib.fc_lcb(uo_docx::doc::fib::fc::Dop);
            let end = u64::from(pair.fc) + u64::from(pair.lcb);
            let dop = streams.table.get(pair.fc as usize..usize::try_from(end).unwrap_or(0)).unwrap_or(&[]);
            black_box(build_settings(dop, streams.fib.n_fib, &fonts, ftc_fe, true, true));
            black_box(read_dop(dop, streams.fib.n_fib));
            let mut r = StyleResolver::new(sheet);
            for i in (0..n).chain([0x0fff, 0xffff]) {
                let i = u16::try_from(i).unwrap_or(u16::MAX);
                black_box(r.resolve(i));
                black_box(r.resolve_char(i));
                let root = r.root_chp.clone();
                black_box(r.apply_char_style(i, &root));
            }
        }
        Err(DocError::Limit { kind: DocLimitKind::StringUnits, max }) => assert_eq!(max, limits.string_budget(streams.table.len() as u64)),
        Err(e) => panic!("{e}"),
    }
    black_box(read_lists(&streams.fib, &streams.table));
}

/// Seeds: the 11 fixtures (both limit selectors) and one in twenty of the mutations of the parity capture.
pub fn doc_seeds(repo: &Path) -> Vec<(&'static str, Vec<u8>)> {
    let mut out = Vec::new();
    let dir = repo.join("write/packages/docx-core/test/fixtures/doc");
    let Ok(rd) = std::fs::read_dir(&dir) else { return out };
    let mut names: Vec<_> = rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "doc")).collect();
    names.sort();
    let with_sel = |sel: u8, b: &[u8]| [&[sel][..], b].concat();
    let mut files = std::collections::HashMap::new();
    for p in &names {
        let Ok(b) = std::fs::read(p) else { continue };
        out.push(("doc_convert", with_sel(0, &b)));
        out.push(("doc_convert", with_sel(1, &b)));
        if let Some(n) = p.file_stem() {
            files.insert(n.to_string_lossy().into_owned(), b);
        }
    }
    let Ok(cap) = std::fs::read_to_string(repo.join("core/crates/uo-docx/tests/parity/doc.capture")) else { return out };
    let mut k = 0;
    for line in cap.lines() {
        let mut fields = line.split('\t');
        let key = fields.next().unwrap_or("");
        let Some(rest) = key.strip_prefix("mut/") else { continue };
        k += 1;
        if k % 20 != 0 {
            continue;
        }
        let Some(base) = files.get(rest.split('/').next().unwrap_or("")) else { continue };
        let facet = |name: &str| line.split('\t').find_map(|f| f.strip_prefix(name).and_then(|f| f.strip_prefix('=')));
        let mut b = base.clone();
        if let Some(edits) = facet("edits").filter(|e| *e != "-") {
            for e in edits.split(',') {
                if let Some((p, v)) = e.split_once(':')
                    && let (Ok(p), Ok(v)) = (p.parse::<usize>(), v.parse::<u8>())
                    && p < b.len()
                {
                    b[p] = v;
                }
            }
        }
        if let Some(cut) = facet("cut").and_then(|c| c.parse::<usize>().ok()) {
            b.truncate(cut);
        }
        out.push(("doc_convert", with_sel(0, &b)));
    }
    out
}
